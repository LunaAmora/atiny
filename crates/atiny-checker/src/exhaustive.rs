//! Does exhaustiveness checking on a [Problem]. The exhaustiveness checking is an algorithm that
//! checks if a pattern match problem is covering all possible cases. This is done by refining a
//! problem into a set of sub problems and checking if the sub problems covers specific cases.

use std::{
    collections::HashSet,
    fmt::{Display, Formatter},
    iter,
    rc::Rc,
};

use atiny_location::ByteRange;
use atiny_tree::elaborated::{CaseTreeNode, Symbol};
use atiny_tree::r#abstract::*;
use itertools::Itertools;

use crate::{context::Ctx, types::*};

/// It is used to return possible constructors of a type. Some types have infinite constructors like
/// Int so we cannot return a list of all the constructors. In that case we return Infinite.
pub enum Finitude<T> {
    Infinite,
    Finite(T),
}

/// A witness is a proof that a pattern match problem is exhaustive. It contains a list of patterns
/// if it's not exhaustive proving it's non exhaustiveness by contradiction. If it's exhaustive then
/// it will return a case tree that contains all the possible cases of the problem.
pub enum Witness {
    Ok(CaseTreeNode),
    NonExhaustive(Row),
}

impl Witness {
    /// Creates a new witness that contains a new pattern based on the thing that got non exhausted.
    pub fn expand(self, size: usize, constructor_name: Option<String>) -> Self {
        if let Self::NonExhaustive(row) = self {
            let (vec, row) = row.split_vec(size);
            let data = match constructor_name {
                Some(name) => PatternKind::Constructor(name, vec),
                _ => PatternKind::Atom(AtomKind::Tuple(vec)),
            };
            let row = row.prepend(Pattern {
                location: ByteRange::singleton(0),
                data,
            });
            Self::NonExhaustive(row)
        } else {
            self
        }
    }

    /// Checks if a witness is non exhaustive.
    pub fn is_non_exhaustive(&self) -> bool {
        matches!(self, Self::NonExhaustive(_))
    }

    /// Adds a pattern to a witness.
    pub fn add_pattern(self, pat: Pattern) -> Self {
        if let Self::NonExhaustive(row) = self {
            Self::NonExhaustive(row.prepend(pat))
        } else {
            self
        }
    }

    pub fn result(self) -> Result<CaseTreeNode, Pattern> {
        match self {
            Self::Ok(res) => Ok(res),
            Self::NonExhaustive(result) => Err(result.into_pattern()),
        }
    }
}

/// A row is a clause of a pattern match. It contains a list of patterns that are being matched.
#[derive(Clone)]
pub struct Row(Option<usize>, Vec<Pattern>);

impl Row {
    /// Checks if a row is empty.
    fn is_empty(&self) -> bool {
        self.1.is_empty()
    }

    /// Adds a sequence of patterns to the beginning of a row and removes the first pattern of the
    /// row. It's used to "inline" a sequence of patterns into a row.
    fn inline<I: IntoIterator<Item = Pattern>>(mut self, vec: I) -> Self {
        self.1 = vec.into_iter().chain(self.1.into_iter().skip(1)).collect();
        self
    }

    /// Removes the first pattern of a row.
    fn pop_front(mut self) -> Self {
        self.1 = self.1.into_iter().skip(1).collect();
        self
    }

    /// Prepends a pattern to a row.
    fn prepend(mut self, pat: Pattern) -> Self {
        self.1 = iter::once(pat).chain(self.1).collect();
        self
    }

    /// Splits a row into two rows. The first row contains the first `place` patterns and the second
    /// row contains the rest of the patterns.
    fn split_vec(mut self, place: usize) -> (Vec<Pattern>, Self) {
        let mut vec = self.1.split_off(place);
        std::mem::swap(&mut vec, &mut self.1);
        (vec, self)
    }

    /// Converts a row into a pattern. This is done by asserting that the row only contains one
    /// pattern and then returning that pattern.
    pub fn into_pattern(self) -> Pattern {
        assert_eq!(self.1.len(), 1);
        self.1.into_iter().next().unwrap()
    }

    pub fn specialize(self, ctx: &Ctx, case: Case<()>) -> Vec<Self> {
        match (self.get_first_pattern(ctx), case) {
            //default_row
            (Case::Wildcard, Case::Wildcard) => vec![self.pop_front()],

            //specialize_constructor
            (Case::Constructor(c1, args), Case::Constructor(c2, _)) if c1.name == c2.name => {
                vec![self.inline(args)]
            }
            (Case::Wildcard, Case::Constructor(constr, _)) => {
                vec![self.inline(vec![wildcard(); constr.args.len()])]
            }

            //specialize_tuple
            (Case::Tuple(p), Case::Tuple(t)) if p.len() == t.len() => vec![self.inline(p)],
            (Case::Wildcard, Case::Tuple(t)) => vec![self.inline(vec![wildcard(); t.len()])],

            //specialize_number
            (Case::Int(n1), Case::Int(n2)) if n1 == n2 => vec![self.pop_front()],
            (Case::Wildcard, Case::Int(_)) => vec![self.pop_front()],

            _ => vec![],
        }
    }

    pub fn is_all_wildcards(&self, ctx: &Ctx) -> bool {
        self.1.iter().all(|pat| {
            matches!(
            &pat.data,
            PatternKind::Atom(AtomKind::Identifier(n)) if ctx.lookup_cons(n).is_none()
            )
        })
    }

    fn first(&self) -> &Pattern {
        self.1.first().unwrap()
    }

    fn get_first_pattern(&self, ctx: &Ctx) -> Case<Pattern> {
        Case::from_pattern(ctx, self.first().data.clone())
    }
}

/// This structure represents if a set of constructors are complete or not.
pub enum Completeness {
    Complete(Vec<String>),
    Incomplete(Finitude<Vec<String>>),
}

#[derive(Clone)]
pub struct Matrix(Vec<Row>);

impl Matrix {
    /// It checks if the matrix is empty (there are no rows inside of it) then it is not exhaustive.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// It transforms a column into a matrix. This is done by transforming each pattern into a row.
    pub fn from_column(column: Vec<Pattern>) -> Self {
        Self(
            column
                .into_iter()
                .enumerate()
                .map(|(place, pat)| Row(Some(place), vec![pat]))
                .collect(),
        )
    }

    /// Checks if the first pattern is a wildcard or a variable so it can determine the set complete.
    pub fn is_wildcard(&self, ctx: &Ctx) -> bool {
        self.0.iter().all(|row| {
            matches!(
            &row.1[0].data,
            PatternKind::Atom(AtomKind::Identifier(n)) if ctx.lookup_cons(n).is_none()
            )
        })
    }

    /// Gets the used constructor of a pattern. This is done by checking if the pattern is a
    /// constructor on the context.
    fn get_used_constructor(ctx: &Ctx, pat: &PatternKind) -> Option<String> {
        match pat {
            PatternKind::Atom(AtomKind::Identifier(id)) => ctx.lookup_cons(id).map(|_| id.clone()),
            PatternKind::Constructor(name, _) => Some(name.clone()),
            _ => None,
        }
    }

    /// Gets the used constructors of a matrix. This is done by getting the used constructors of
    /// each of the rows.
    fn get_used_constructors(&self, ctx: &Ctx) -> Vec<String> {
        self.0
            .iter()
            .flat_map(|row| Self::get_used_constructor(ctx, &row.first().data))
            .collect()
    }

    pub fn is_complete_type_sig(&self, ctx: &Ctx, type_sig: &TypeSignature) -> Completeness {
        let names = type_sig.get_constructors();

        names.map_or(Completeness::Incomplete(Finitude::Infinite), |names| {
            let used_constructors = self
                .get_used_constructors(ctx)
                .into_iter()
                .filter(|name| names.contains(name.as_str()))
                .collect::<HashSet<_>>();

            if used_constructors.len() == names.len() || self.is_wildcard(ctx) {
                Completeness::Complete(used_constructors.into_iter().collect())
            } else {
                Completeness::Incomplete(Finitude::Finite(
                    names.difference(&used_constructors).cloned().collect(),
                ))
            }
        })
    }

    /// If removes all the rows that are after a row that is completely made out of wildcards or
    /// variables. It is just an optimization.
    pub fn filter_first_match(&mut self, ctx: &Ctx) {
        let index = self.0.iter().find_position(|row| row.is_all_wildcards(ctx));
        if let Some((index, _)) = index {
            self.0.truncate(index + 1);
        }
    }

    /// Creates a new matrix where all the rows are specialized by a [`Case`]. This is done by
    /// checking if each row matches a [`Case`] in the first pattern and then flattening all
    /// the patterns into the row or removing it if it does not matches.
    pub fn specialize(self, ctx: &Ctx, case: Case<()>) -> Self {
        Self(
            self.0
                .into_iter()
                .flat_map(|row| row.specialize(ctx, case.clone()))
                .collect(),
        )
    }
}

/// A problem is something that need to solve in order to check if a pattern match is exhaustive.
/// it contains the type of the problem, the case that is being checked and the rows that are the
/// patterns that are being matched.
#[derive(Clone)]
pub struct Problem {
    typ: Vec<Type>,
    case: Row,
    matrix: Matrix,
}

impl Display for Row {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.1.is_empty() {
            write!(f, "No Row")
        } else {
            write!(f, "| {}", self.1[0])?;
            for pat in self.1.iter().skip(1) {
                write!(f, " {}", pat)?;
            }
            Ok(())
        }
    }
}

impl Display for Problem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Problem:")?;
        for typ in self.typ.iter() {
            write!(f, " {}", typ)?;
        }
        writeln!(f)?;
        writeln!(f, "Case: \n  {}", self.case)?;
        writeln!(f, "Rows:")?;
        for row in self.matrix.0.iter() {
            writeln!(f, "  {}", row)?;
        }
        Ok(())
    }
}

impl Problem {
    /// Creates a new problem. It takes the scrutineer type of the problem, the case that is being
    /// checked and the patterns that are being matched.
    pub fn new(typ: Type, useful: Vec<Pattern>, columns: Vec<Pattern>) -> Self {
        Self {
            typ: vec![typ],
            case: Row(None, useful),
            matrix: Matrix::from_column(columns),
        }
    }

    /// Checks if the matrix is exhaustive by looking at all the rows and checking if any of them
    /// is empty (it matches ).
    fn get_exhaustive_row(&self) -> Option<&Row> {
        self.matrix.0.iter().find(|row| row.is_empty())
    }

    /// Checks if the problem is empty. A problem is empty if the matrix is empty.
    pub fn is_empty(&self) -> bool {
        self.matrix.is_empty()
    }

    /// Specializes into a new problem. This is done by instantiating the type of the problem with
    /// the type arguments and then flattening the rest of the type arguments into the type of the
    /// problem and specializing each of the rows with a constructor.
    fn specialize_cons(
        self,
        ctx: &Ctx,
        type_args: &[Type],
        constructor: Rc<ConstructorSignature>,
        pat: Vec<Pattern>,
    ) -> Witness {
        let generalized = constructor.typ.instantiate_with(type_args);
        let arg_count = constructor.args.len();
        let case = Case::Constructor(constructor, vec![(); arg_count]);
        self.specialize(ctx, generalized.iter(), pat, case)
    }

    /// Splits a problem into multiple by looking at the type of the problem and the name of the
    /// type and then looking at all the constructors of the type and getting a result of each of
    /// them.
    fn split_on_defined_cons(self, ctx: &Ctx, name: &str, type_args: &[Type]) -> Witness {
        let type_sig = ctx
            .lookup_type(name)
            .unwrap_or_else(|| panic!("cannot find type '{name}'"));

        match type_sig.value {
            TypeValue::Sum(sum) => {
                let mut nodes = Vec::new();

                for constructor in sum {
                    let len = constructor.args.len();
                    let witness = self.clone().specialize_cons(
                        ctx,
                        type_args,
                        constructor.clone(),
                        vec![wildcard(); len],
                    );

                    let name = &constructor.name;

                    if witness.is_non_exhaustive() {
                        return witness.expand(len, Some(name.clone()));
                    } else if let Witness::Ok(tree) = witness {
                        nodes.push((Symbol(name.clone()), tree));
                    }
                }

                Witness::Ok(CaseTreeNode::Node(nodes))
            }

            // Opaque types will never be splitted because they are either incomplete or
            // they fall in the is_all_wildcards case
            TypeValue::Opaque => unreachable!("Opaque types are impossible here"),
            TypeValue::Product(_) => unreachable!("Record types are impossible here"),
        }
    }

    /// Returns the first type of the problem.
    fn current_type(&self) -> Type {
        self.typ.first().cloned().unwrap().flatten()
    }

    /// Creates a new problem where the first type is removed and the case is updated to match the
    /// first type. It's used when we need to specialize the tree to an identifier
    fn default_matrix(self, ctx: &Ctx) -> Self {
        Self {
            typ: self.typ[1..].to_vec(),
            case: self.case.pop_front(),
            matrix: self.matrix.specialize(ctx, Case::Wildcard),
        }
    }

    /// Generates a constructor pattern for the given constructor name.
    fn synthesize_constructor(&self, ctx: &Ctx, name: &str) -> Pattern {
        let cons_sig = ctx.lookup_cons(name).unwrap();

        let args = vec![wildcard(); cons_sig.args.len()];

        Pattern {
            location: ByteRange::default(),
            data: PatternKind::Constructor(name.to_string(), args),
        }
    }

    /// Checks if all the rows are wildcard.
    fn is_all_wildcard(&self, ctx: &Ctx) -> bool {
        self.matrix.is_wildcard(ctx)
    }

    /// Checks if the problem is exhaustive for a wildcard. This is done by checking if the matrix
    /// is complete for the current type (it means that it contains all of the constructors as
    /// wildcards)
    fn exhaustiveness_wildcard(
        self,
        ctx: &Ctx,
        name: &str,
        type_args: &[Type],
        current_typ: TypeSignature,
    ) -> Witness {
        // If the first column is wildcard, then we can just return the [Problem::default_matrix]
        // that specializes the tree for identifiers removing all constructor rows.
        if self.is_all_wildcard(ctx) {
            self.specialize_wildcard(ctx)
        } else {
            // If it's not all wildcard, then we need to check if the matrix is complete for the
            // current type. If it is, then we need to split the problem into multiple problems.
            match self.matrix.is_complete_type_sig(ctx, &current_typ) {
                // If it's complete then we split for all of the possible constructors. It's done
                // to ensure that all of the possible matrices are checked.
                Completeness::Complete(_) => self.split_on_defined_cons(ctx, name, type_args),
                // If it's incomplete and infinite, then we can just return the default_matrix with
                // a wildcard that represents all of the missing constructors.
                Completeness::Incomplete(Finitude::Infinite) => self.specialize_wildcard(ctx),
                // If it's incomplete, then we need to synthesize a constructor pattern and add it
                // in order to generate a case that contains the missing constructor.
                Completeness::Incomplete(Finitude::Finite(constructors)) => {
                    let first = self.synthesize_constructor(ctx, constructors.first().unwrap());
                    // Removes the first column from the matrix because we know it's incomplete.
                    let witness = self.default_matrix(ctx).exhaustiveness(ctx);
                    witness.add_pattern(first)
                }
            }
        }
    }

    fn specialize_wildcard(self, ctx: &Ctx) -> Witness {
        let witness = self.default_matrix(ctx).exhaustiveness(ctx);
        witness.add_pattern(wildcard())
    }

    fn specialize<T, P>(self, ctx: &Ctx, types: T, pat: P, case: Case<()>) -> Witness
    where
        T: IntoIterator<Item = Type>,
        P: IntoIterator<Item = Pattern>,
    {
        let specialized = Self {
            typ: types
                .into_iter()
                .chain(self.typ.iter().skip(1).cloned())
                .collect(),
            case: self.case.inline(pat),
            matrix: self.matrix.specialize(ctx, case),
        };

        specialized.exhaustiveness(ctx)
    }

    /// Checks if the problem is exhaustive on a case. This is the "entrypoint" forall of the
    /// exhaustiveness checking and it works by getting a [Problem] and checking some things about
    /// it's structure.
    ///
    /// 1. If the problem is empty, then it is non-exhaustive because a problem was
    /// specialized into a form that is not exhaustive. e.g
    ///
    /// ```md
    /// match some_value {}
    /// ```
    ///
    /// This code above is non exhaustive if the type of `some_value` is `Option<T>`. Because it
    /// lacks two cases, `Some` and `None`. So `is_empty` means that there are no cases that match
    /// this specific problem.
    ///
    /// 2. The problem is exhaustive if there are empty rows, It means that we specialized the tree
    /// so much that this row was not removed and it matches the problem case. e.g
    ///
    /// ```md
    /// match some_value {
    ///    Some(_) => {}
    ///    None => {}
    /// }
    /// ```
    ///
    /// That the corresponding problem is:
    ///
    /// ```    /// Type: T
    /// Case: _
    /// Rows:
    ///     | _
    ///
    /// Type: Option<T>
    /// Case: _
    /// Rows:
    ///     | (Some _)
    ///     | None
    /// ```
    ///
    /// Will be specialized into two [Problem]s, one for `Some` and other for `None`. The `Some`
    /// problem will have an empty row, because we specialized both rows for `Some` and then the
    /// first row is inlined resulting a problem like
    ///
    /// ```js
    /// Type: T
    /// Case: _
    /// Rows:
    ///     | _
    ///
    /// // That will get specialized into
    ///
    /// Type:
    /// Case:
    /// Rows:
    ///     |
    ///
    /// ```
    ///
    /// This is the exhaustive case, because we specialized the problem into a form that contains at
    /// least one empty row.
    ///
    /// 3. The incomplete matrix case that is the most common case. It means that we have a problem
    /// that is not empty and not exhaustive. e.g
    ///
    /// ```md
    /// Type: Option<T>
    /// Case: _
    /// Rows:
    ///     | (Some _)
    /// ```
    ///
    /// And we need to check if it's complete or not. To do that we need to analyze the case and how
    /// it's going to force the type to be specialized. In this case, the case is `_` that means
    /// that we need to check if the type is complete for the `_` case using the function
    /// [Problem::exhaustiveness_wildcard].
    pub fn exhaustiveness(self, ctx: &Ctx) -> Witness {
        // It's non exhaustive if the current problem is empty (it means that there are no rows that
        // matches the case)
        if self.is_empty() {
            Witness::NonExhaustive(self.case)
        } else if let Some(row) = self.get_exhaustive_row().and_then(|x| x.0) {
            Witness::Ok(CaseTreeNode::Leaf(row))
        } else {
            // The first pattern of the case will guide the specialization of the whole problem.
            let pat = self.case.first().clone().data;
            self.match_exhaustiveness(ctx, Case::from_pattern(ctx, pat))
        }
    }

    pub fn match_exhaustiveness(self, ctx: &Ctx, case: Case<Pattern>) -> Witness {
        match (case, &*self.current_type()) {
            (Case::Wildcard, MonoType::Application(n, args)) => match ctx.lookup_type(n) {
                Some(sig) => self.exhaustiveness_wildcard(ctx, n, args, sig),
                None => self.specialize_wildcard(ctx),
            },

            (Case::Wildcard, MonoType::Tuple(types)) => {
                let len = types.len();
                let witness = self.specialize(
                    ctx,
                    types.clone(),
                    vec![wildcard(); len],
                    Case::Tuple(vec![(); len]),
                );

                if witness.is_non_exhaustive() {
                    witness.expand(len, None)
                } else {
                    witness
                }
            }

            (Case::Wildcard, _) => self.specialize_wildcard(ctx),

            (Case::Constructor(cons, pat), MonoType::Application(_, args)) => {
                self.specialize_cons(ctx, args, cons, pat)
            }

            (Case::Tuple(pat), MonoType::Tuple(types)) => {
                self.specialize(ctx, types.clone(), pat, Case::Tuple(vec![(); types.len()]))
            }

            (Case::Int(n), _) => self.specialize(ctx, None, None, Case::Int(n)),

            _ => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Case<T: Clone> {
    Constructor(Rc<ConstructorSignature>, Vec<T>),
    Tuple(Vec<T>),
    Int(u64),
    Wildcard,
}

impl Case<Pattern> {
    fn from_pattern(ctx: &Ctx, pat: PatternKind) -> Self {
        match pat {
            PatternKind::Atom(atom) => Self::from_atom(ctx, atom),
            PatternKind::Constructor(name, args) => {
                let cons = ctx.lookup_cons(&name).unwrap();
                Self::Constructor(cons, args)
            }
        }
    }

    fn from_atom(ctx: &Ctx, atom: AtomKind<Pattern>) -> Self {
        match atom {
            AtomKind::Wildcard => Self::Wildcard,
            AtomKind::Identifier(name) => ctx
                .lookup_cons(&name)
                .map_or_else(|| Self::Wildcard, |cons| Self::Constructor(cons, vec![])),
            AtomKind::Number(number) => Self::Int(number),
            AtomKind::Tuple(tuple) => Self::Tuple(tuple),
            AtomKind::Path(_, _) => todo!(),
        }
    }
}
