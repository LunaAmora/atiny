//! This module describes the first tree that is generated from the parser. It's a tree that contains
//! pure syntactic information due the origin. The main types of this file are [Expr], [Pattern] and
//! [TopLevel].

use std::fmt::{self, Display};

use atiny_location::{Byte, ByteRange, Located};
use itertools::Itertools;

use crate::SeqHelper;

#[derive(Debug, Clone)]
pub struct Qualifier(pub Vec<Located<String>>);

impl Display for Qualifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.iter().join("::"))
    }
}

/// Primary expressions that are used both for [Pattern] and [Expr].
#[derive(Debug, Clone)]
pub enum AtomKind<T> {
    Wildcard,
    Number(u64),
    Tuple(Vec<T>),
    Identifier(String),
    Path(Qualifier, Located<String>),
}

impl<T> AtomKind<T> {
    pub fn unit() -> Self {
        Self::Identifier("()".to_string())
    }
}

impl<T: Display> Display for AtomKind<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => write!(f, "_"),
            Self::Number(n) => write!(f, "{n}"),
            Self::Tuple(t) => write!(f, "({})", t.iter().join(", ")),
            Self::Identifier(id) => write!(f, "{id}"),
            Self::Path(q, s) => write!(f, "{q}.{s}"),
        }
    }
}

impl From<AtomKind<Expr>> for ExprKind {
    fn from(value: AtomKind<Expr>) -> Self {
        Self::Atom(value)
    }
}

impl From<AtomKind<Pattern>> for PatternKind {
    fn from(value: AtomKind<Pattern>) -> Self {
        Self::Atom(value)
    }
}

#[derive(Debug)]
pub struct ExprField {
    pub name: String,
    pub expr: Expr,
}

impl Display for ExprField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { name, expr: value } = self;
        write!(f, "{name} = {value}")
    }
}

#[derive(Debug)]
pub enum StatementKind {
    Let(Pattern, Expr),
    Expr(Expr),
}

impl Display for StatementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Let(n, v) => write!(f, "let {n} = {v}"),
            Self::Expr(e) => write!(f, "{e}"),
        }
    }
}

pub type Statement = Located<StatementKind>;

/// Expressions are language constructions that intrinsically contains a return value. E.g
///
/// ```atiny
/// let a = 2; b // b is the return value of the hole expression.
/// ```
///
#[derive(Debug)]
pub enum ExprKind {
    Atom(AtomKind<Expr>),
    Match(Box<Expr>, Vec<Clause>),
    Abstraction(String, Box<Expr>),
    Application(Box<Expr>, Box<Expr>),
    Annotation(Box<Expr>, Box<TypeNode>),
    RecordCreation(Box<Expr>, Vec<ExprField>),
    Field(Box<Expr>, String),
    Block(Vec<Statement>),
}

impl Display for ExprKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atom(a) => write!(f, "{}", a),
            Self::Abstraction(p, e) => write!(f, "(|{p}| {e})"),
            Self::Annotation(p, e) => write!(f, "({p} : {e})"),
            Self::Application(fu, a) => write!(f, "({fu} {a})"),
            Self::Match(e, c) => write!(f, "{e} {{{}}}", c.iter().join(", ")),
            Self::RecordCreation(n, fields) => write!(f, "{n} {{ {} }}", fields.iter().join(", ")),
            Self::Field(e, n) => write!(f, "{e}.{n}"),
            Self::Block(b) => write!(f, "{{{}}}", b.iter().join("; ")),
        }
    }
}

impl ExprKind {
    pub fn if_let(pattern: Pattern, matcher: Expr, true_arm: Expr, else_arm: Expr) -> Self {
        let else_pat = Pattern {
            location: else_arm.location,
            data: PatternKind::Atom(AtomKind::Wildcard),
        };

        Self::Match(
            Box::new(matcher),
            vec![
                Clause::new(pattern, true_arm),
                Clause::new(else_pat, else_arm),
            ],
        )
    }

    pub fn infix<T: Display>(left: Expr, infix: Located<T>, right: Expr) -> Self {
        let location = ByteRange(left.location.0, infix.location.1);
        let call = infix.map(|i| Self::Atom(AtomKind::Identifier(i.to_string())));

        let data = Self::Application(Box::new(call), Box::new(left));
        let app = Located { data, location };

        Self::Application(Box::new(app), Box::new(right))
    }
}

/// An atiny expression is some syntactic element that intrinsically has a value. It can be a literal
/// value, a variable, a function call, a function definition and etc.
pub type Expr = Located<ExprKind>;

/// A pattern is a syntactic element that goes inside pattern match declarations and are used to
/// match on values and destruct them.
#[derive(Debug, Clone)]
pub enum PatternKind {
    Atom(AtomKind<Pattern>),
    Constructor(String, Vec<Pattern>),
}

impl Display for PatternKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atom(i) => write!(f, "{}", i),
            Self::Constructor(name, args) if args.is_empty() => {
                write!(f, "{}", name)
            }
            Self::Constructor(name, args) => {
                write!(f, "{name}{}", args.iter().map(|x| format!(" {x}")).join(""))
            }
        }
    }
}

pub type Pattern = Located<PatternKind>;

impl PatternKind {
    pub fn is_variable(&self) -> bool {
        matches!(self, Self::Atom(AtomKind::Identifier(_)))
    }
}

/// Creates a wildcard pattern
pub fn wildcard() -> Pattern {
    Pattern {
        location: ByteRange(Byte(0), Byte(0)),
        data: PatternKind::Atom(AtomKind::Wildcard),
    }
}

/// This is a clause of a pattern match declaration. It contains a pattern and an expression that is
/// the result of the match.
#[derive(Debug)]
pub struct Clause {
    pub pat: Pattern,
    pub expr: Expr,
}

impl Clause {
    pub fn new(pat: Pattern, expr: Expr) -> Self {
        Self { pat, expr }
    }
}

impl Display for Clause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} => {}", self.pat, self.expr)
    }
}

#[derive(Debug)]
pub struct ArrowNode {
    pub left: Box<TypeNode>,
    pub right: Box<TypeNode>,
}

impl ArrowNode {
    pub fn new(left: TypeNode, right: TypeNode) -> Self {
        Self {
            left: Box::new(left),
            right: Box::new(right),
        }
    }
}

impl Display for ArrowNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({} -> {})", self.left, self.right)
    }
}

#[derive(Debug)]
pub struct VariableNode {
    pub name: String,
}

impl Display for VariableNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Universal quantification as a type
#[derive(Debug)]
pub struct ForallNode {
    pub args: Vec<String>,
    pub body: Box<TypeNode>,
}

impl Display for ForallNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args = self.args.iter().join(" ");
        write!(f, "(forall ({}) . {})", args, self.body)
    }
}

#[derive(Debug)]
pub struct TypeApplicationNode {
    pub fun: String,
    pub args: Vec<TypeNode>,
}

impl Display for TypeApplicationNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args = self.args.iter().join(" ");
        write!(f, "({} {})", self.fun, args)
    }
}

#[derive(Debug)]
pub struct TypeTupleNode {
    pub types: Vec<TypeNode>,
}

impl Display for TypeTupleNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})", self.types.iter().join(", "))
    }
}

/// The representation of a type in the AST.
#[derive(Debug)]
pub enum TypeKind {
    Arrow(ArrowNode),
    Variable(VariableNode),
    Forall(ForallNode),
    Application(TypeApplicationNode),
    Tuple(TypeTupleNode),
}

impl TypeKind {
    pub fn unit() -> Self {
        Self::Variable(VariableNode {
            name: "()".to_string(),
        })
    }
}

impl Display for TypeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arrow(n) => write!(f, "{}", n),
            Self::Variable(n) => write!(f, "{}", n),
            Self::Forall(n) => write!(f, "{}", n),
            Self::Application(n) => write!(f, "{}", n),
            Self::Tuple(n) => write!(f, "{}", n),
        }
    }
}

pub type TypeNode = Located<TypeKind>;

#[derive(Debug)]
pub struct Constructor {
    pub name: String,
    pub types: Vec<TypeNode>,
}

impl Display for Constructor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "| {} {}",
            self.name,
            self.types.iter().map(|x| format!("({})", x)).join(" ")
        )
    }
}
#[derive(Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeNode,
}

#[derive(Debug)]
pub enum TypeDeclKind {
    Sum(Vec<Constructor>),
    Product(Vec<Field>),
}

#[derive(Debug)]
pub struct TypeDecl {
    pub name: String,
    pub params: Vec<String>,
    pub constructors: TypeDeclKind,
}

impl TypeDecl {
    pub fn unit() -> Self {
        Self {
            name: "()".to_string(),
            params: Vec::new(),
            constructors: TypeDeclKind::Sum(vec![Constructor {
                name: "()".to_string(),
                types: Vec::new(),
            }]),
        }
    }
}

impl Display for TypeDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params = self.params.iter().map(|x| format!(" {x}")).join("");

        match &self.constructors {
            TypeDeclKind::Sum(constructors) => write!(
                f,
                "(type {name}{params} (sum {constructors}))",
                name = self.name,
                params = params,
                constructors = constructors.iter().join(" ")
            ),
            TypeDeclKind::Product(fields) => write!(
                f,
                "(type {name}{params} (product {fields}))",
                name = self.name,
                params = params,
                fields = fields
                    .iter()
                    .map(|x| format!("({} : {})", x.name, x.ty))
                    .join(" ")
            ),
        }
    }
}

#[derive(Debug)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<(Pattern, TypeNode)>,
    pub ret: TypeNode,
    pub body: Expr,
}

impl FnDecl {
    pub fn new(
        name: Located<String>,
        mut params: Vec<(Pattern, TypeNode)>,
        ret: Option<TypeNode>,
        body: Expr,
    ) -> Self {
        let loc = name.location;
        if params.is_empty() {
            params = vec![(
                Located::new(loc, PatternKind::Atom(AtomKind::Wildcard)),
                Located::new(loc, TypeKind::unit()),
            )];
        }

        Self {
            name: name.data,
            params,
            ret: ret.unwrap_or_else(|| Located::new(loc, TypeKind::unit())),
            body,
        }
    }
}

impl Display for FnDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params = self
            .params
            .iter()
            .map(|(n, t)| format!("({} : {})", n, t))
            .join(" ");

        write!(
            f,
            "fn {} {} : {} {}",
            self.name, params, self.ret, self.body
        )
    }
}

#[derive(Debug)]
pub struct UseDecl(pub Qualifier, pub Option<Located<String>>);

impl Display for UseDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        if let Some(a) = &self.1 {
            write!(f, ".{}", a)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum TopLevelKind {
    TypeDecl(TypeDecl),
    FnDecl(FnDecl),
    UseDecl(UseDecl),
}

impl SeqHelper<TopLevel> for TypeDecl {
    fn is_variant(enumerator: &TopLevel) -> bool {
        matches!(enumerator.data, TopLevelKind::TypeDecl(_))
    }

    fn get_variant(enumerator: TopLevel) -> Option<Self> {
        match enumerator.data {
            TopLevelKind::TypeDecl(d) => Some(d),
            _ => None,
        }
    }
}

impl SeqHelper<TopLevel> for FnDecl {
    fn is_variant(enumerator: &TopLevel) -> bool {
        matches!(enumerator.data, TopLevelKind::FnDecl(_))
    }

    fn get_variant(enumerator: TopLevel) -> Option<Self> {
        match enumerator.data {
            TopLevelKind::FnDecl(d) => Some(d),
            _ => None,
        }
    }
}

impl SeqHelper<TopLevel> for UseDecl {
    fn is_variant(enumerator: &TopLevel) -> bool {
        matches!(enumerator.data, TopLevelKind::UseDecl(_))
    }

    fn get_variant(enumerator: TopLevel) -> Option<Self> {
        match enumerator.data {
            TopLevelKind::UseDecl(d) => Some(d),
            _ => None,
        }
    }
}

/// It's a declaration on the top level of the program. It can be a function definition, a type
/// definition, a trait definition, record definition and etc.
pub type TopLevel = Located<TopLevelKind>;

impl Display for TopLevelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeDecl(td) => write!(f, "{}", td),
            Self::FnDecl(fd) => write!(f, "{}", fd),
            Self::UseDecl(ud) => write!(f, "{}", ud),
        }
    }
}
