//! Type inference for for expressions.

use super::Infer;
use crate::check::Check;
use crate::context::{Ctx, InferError};
use crate::exhaustive::Problem;
use crate::types::{MonoType, Type, TypeScheme, TypeSignature, TypeValue};
use crate::unify::unify;

use atiny_error::SugestionKind;
use atiny_tree::elaborated::{self, CaseTree, Stmt, Symbol, VariableNode};
use atiny_tree::r#abstract::*;

use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type Elaborated = elaborated::Expr<Type>;

impl Infer for &Expr {
    type Context<'a> = Ctx;
    type Return = (Type, Elaborated);

    fn infer(self, mut ctx: Self::Context<'_>) -> Self::Return {
        use AtomKind::*;
        use ExprKind::*;
        ctx.set_position(self.location);

        match &self.data {
            Atom(a) => match a {
                Wildcard => ctx.new_error("`_` may only appear on patterns".to_string()),

                Number(n) => (MonoType::typ("Int".to_string()), Elaborated::Number(*n)),

                Tuple(vec) => {
                    let (typ, el) = vec.iter().map(|expr| expr.infer(ctx.clone())).unzip();
                    (MonoType::tuple(typ), Elaborated::Tuple(el))
                }

                Identifier(x) => match ctx.lookup(x) {
                    Some(sigma) => {
                        let (inst, inst_types) = sigma.instantiate(ctx);

                        let variable_node = VariableNode {
                            inst_types,
                            name: Symbol(x.to_owned()),
                        };

                        (inst, Elaborated::Variable(variable_node))
                    }

                    None => ctx.new_error(format!("unbound variable '{}'", x)),
                },

                Path(_, _) => todo!(),
            },

            Application(fun, arg) => {
                let (t0, mut elab_fun) = fun.infer(ctx.clone());
                let (t1, elab_arg) = arg.infer(ctx.clone());

                let t_ret = ctx.new_hole();
                let function_type = t1.arrow(t_ret.clone());

                unify(ctx, t0, function_type);

                let appl = match elab_fun {
                    Elaborated::Application(_, ref mut args, _) => {
                        args.push(elab_arg);
                        elab_fun
                    }
                    _ => Elaborated::Application(Box::new(elab_fun), vec![elab_arg], t_ret.clone()),
                };

                (t_ret, appl)
            }

            Abstraction(param, body) => {
                let t = ctx.new_hole();
                let new_ctx = ctx.extend(param.to_owned(), t.to_poly());
                let (t_line, mut elab_body) = body.infer(new_ctx);

                let symbol = Symbol(param.to_owned());

                let abs = match elab_body {
                    Elaborated::Abstraction(ref mut args, _) => {
                        args.push_back(symbol);
                        elab_body
                    }
                    _ => Elaborated::Abstraction([symbol].into(), Box::new(elab_body)),
                };

                (t.arrow(t_line), abs)
            }

            Match(e, clauses) => {
                let err_count = ctx.err_count();

                let (pat_ty, scrutinee) = e.infer(ctx.clone());
                let ret_ty = ctx.new_hole();

                let mut places = Vec::new();

                for c in clauses {
                    let mut set = HashSet::new();
                    let pat_loc = c.pat.location;

                    let clause_pat = c.pat.clone().infer((&mut ctx, &mut set));
                    ctx.set_position(pat_loc);

                    unify(ctx.clone(), pat_ty.clone(), clause_pat);

                    let (right, elaborated) = c.expr.infer(ctx.clone());

                    unify(ctx.clone(), ret_ty.clone(), right);
                    places.push(elaborated);
                }

                // We can't do coverage checking / exhaustiveness checking without a well typed
                // pattern match and with linear variables.
                let elaborated = if err_count == ctx.err_count() {
                    let columns = if !clauses.is_empty() {
                        let mut columns = vec![clauses[0].pat.clone()];

                        for c in &clauses[1..] {
                            let column_pat = c.pat.clone();
                            let column = vec![column_pat.clone()];
                            let problem = Problem::new(pat_ty.clone(), column, columns.clone());

                            ctx.set_position(column_pat.location);
                            let witness = problem.exhaustiveness(&ctx);

                            if !witness.is_non_exhaustive() {
                                ctx.error(format!("the clause is useless: {}", column_pat));
                            }

                            columns.push(column_pat.clone());
                        }

                        columns
                    } else {
                        Vec::new()
                    };

                    let problem = Problem::new(pat_ty, vec![wildcard()], columns);
                    let witness = problem.exhaustiveness(&ctx);

                    witness.result().map_or_else(
                        |err| {
                            let last_pat_loc = ctx.location;
                            ctx.set_position(e.location);
                            ctx.error(format!("non-exhaustive pattern match: {}", err));

                            ctx.set_position(last_pat_loc);
                            ctx.suggestion(format!("{} => _,", err), SugestionKind::Insert);
                            Elaborated::Error
                        },
                        |tree| Elaborated::CaseTree(Box::new(scrutinee), CaseTree { tree, places }),
                    )
                } else {
                    Elaborated::Error
                };

                (ret_ty, elaborated)
            }

            Annotation(expr, typ) => {
                let typ_res = typ.infer(ctx.clone());
                let elab = expr.check(ctx, typ_res.clone());
                (typ_res, elab)
            }

            RecordCreation(expr, user_fields) => match &expr.data {
                Atom(AtomKind::Identifier(name)) if ctx.lookup_type(name).is_some() => {
                    let err_count = ctx.err_count();
                    ctx.set_position(expr.location);
                    let typ = ctx.lookup_type(name).unwrap();

                    let Some((record, ret_type)) =
                        ctx.inst_sig_as_record(typ.clone(), name.to_owned())
                    else {
                        return ctx.new_error(format!("the type '{typ}' is not a record"));
                    };

                    let elab_fields = user_fields.infer((ctx.clone(), record, true));

                    let elaborated = if err_count != ctx.err_count() {
                        Elaborated::Error
                    } else {
                        Elaborated::RecordCreation(Symbol(name.to_owned()), elab_fields)
                    };

                    (ret_type, elaborated)
                }

                _ => {
                    let err_count = ctx.err_count();
                    let (expr_ty, elab_expr) = expr.infer(ctx.clone());

                    let Some((record, ret_type)) = ctx.as_record_info(&expr_ty) else {
                        ctx.error(format!("the type '{expr_ty}' is not a record"));
                        return (expr_ty, Elaborated::Error);
                    };

                    unify(ctx.clone(), ret_type.clone(), expr_ty);

                    let elab_fields = user_fields.infer((ctx.clone(), record, false));

                    let elaborated = if err_count != ctx.err_count() {
                        Elaborated::Error
                    } else {
                        Elaborated::RecordUpdate(Box::new(elab_expr), elab_fields)
                    };

                    (ret_type, elaborated)
                }
            },

            Field(expr, field) => {
                let (expr_ty, elab_expr) = expr.infer(ctx.clone());
                ctx.set_position(expr.location);

                let Some((record, ret_type)) = ctx.as_record_info(&expr_ty) else {
                    return ctx.new_error(format!("the type '{expr_ty}' is not a record"));
                };

                let Some((_, field_cons)) = record.fields.iter().find(|(name, _)| name == field)
                else {
                    return ctx.new_error(format!(
                        "field '{field}' does not exist in type '{}'",
                        record.id
                    ));
                };

                unify(ctx.clone(), ret_type, expr_ty);

                let field_ty = TypeScheme {
                    names: record.params.to_vec(),
                    mono: field_cons.clone(),
                }
                .instantiate_with(&record.vars);

                let record_field = Elaborated::RecordField(
                    Symbol(record.id),
                    Box::new(elab_expr),
                    Symbol(field.clone()),
                );

                (field_ty, record_field)
            }

            Block(statements) => statements.infer(ctx),
        }
    }
}

impl Infer for &[Statement] {
    type Context<'a> = Ctx;
    type Return = (Type, Elaborated);

    fn infer(self, mut ctx: Self::Context<'_>) -> Self::Return {
        let mut elaborated = Vec::with_capacity(self.len());
        let mut last = None;

        for stmt in self.iter() {
            match &stmt.data {
                StatementKind::Let(pat, exp) => {
                    let (typ, elab) = exp.infer(ctx.level_up());
                    let witness = ctx.single_exhaustiveness(pat, typ);

                    match witness.result() {
                        Err(err) => {
                            ctx.set_position(pat.location);
                            ctx.error("refutable pattern in let statement. Consider using `match` instead".to_string());

                            ctx.set_position(stmt.location);
                            ctx.suggestions(
                                vec![
                                    format!("match {exp} {{"),
                                    format!("    {pat} => _,"),
                                    format!("    {err} => _,"),
                                ],
                                SugestionKind::Replace,
                            );
                        }

                        Ok(tree) => elaborated.push(Stmt::Let(tree, elab)),
                    }

                    last = None;
                }

                StatementKind::Expr(expr) => {
                    let (typ, elab) = expr.infer(ctx.clone());
                    elaborated.push(Stmt::Expr(elab));
                    last = Some(typ);
                }
            }
        }

        let Some(ret) = last else {
            todo!("report that let statements cant be at the end of a block");
        };

        (ret, Elaborated::Block(elaborated))
    }
}

impl InferError<(Type, Elaborated)> for Ctx {
    fn new_error(&self, msg: String) -> (Type, Elaborated) {
        self.error(msg);
        (Rc::new(MonoType::Error), Elaborated::Error)
    }
}

impl Infer for &[ExprField] {
    type Context<'a> = (Ctx, RecordInfo, bool);
    type Return = Vec<(Symbol, Elaborated)>;

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (mut ctx, record, exaustive) = ctx;

        let fields_map: HashMap<_, _> = record.fields.iter().map(|(s, t)| (s, t)).collect();
        let mut fields_to_remove: HashSet<_> = fields_map.keys().collect();
        let mut elab_fields = vec![];

        for ExprField { name, expr } in self {
            let (field_ty, field_expr) = expr.infer(ctx.clone());

            if fields_map.contains_key(name) {
                if fields_to_remove.contains(&name) {
                    ctx.set_position(expr.location);

                    fields_to_remove.remove(&name);

                    let field = fields_map[name];

                    let field = TypeScheme {
                        names: record.params.to_vec(),
                        mono: field.clone(),
                    }
                    .instantiate_with(&record.vars);

                    unify(ctx.clone(), field_ty, field.clone());

                    elab_fields.push((Symbol(name.to_owned()), field_expr));
                } else {
                    ctx.error(format!("field '{name}' is duplicated"));
                }
            } else {
                ctx.error(format!(
                    "field '{name}' does not exist in type '{}'",
                    record.id
                ));
            }
        }

        if exaustive && !fields_to_remove.is_empty() && !fields_map.is_empty() {
            ctx.error(format!(
                "fields {} are missing",
                fields_to_remove.iter().join(", ")
            ));
        }

        elab_fields
    }
}

pub struct RecordInfo {
    id: String,
    fields: Vec<(String, Type)>,
    params: Vec<String>,
    vars: Vec<Type>,
}

impl Ctx {
    fn as_record_info(&self, expr_ty: &Type) -> Option<(RecordInfo, Type)> {
        expr_ty.get_constructor().and_then(|name| {
            self.lookup_type(&name)
                .and_then(|typ| self.inst_sig_as_record(typ, name))
        })
    }

    fn inst_sig_as_record(&self, sig: TypeSignature, id: String) -> Option<(RecordInfo, Type)> {
        let mono = sig.application();
        let TypeSignature { params, value, .. } = sig;

        let TypeValue::Product(fields) = value else {
            return None;
        };

        let (ret_type, vars) = TypeScheme {
            names: params.clone(),
            mono,
        }
        .instantiate(self.clone());

        let record = RecordInfo {
            id,
            fields,
            params,
            vars,
        };

        Some((record, ret_type))
    }
}
