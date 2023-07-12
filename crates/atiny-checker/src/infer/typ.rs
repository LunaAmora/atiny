//! Type inference for types on type annotations.

use super::Infer;
use crate::{context::*, types::*};

use atiny_tree::r#abstract::{TypeKind, TypeNode};
use std::rc::Rc;

impl Infer for &TypeNode {
    type Context<'a> = Ctx;
    type Return = Type;

    fn infer(self, mut ctx: Self::Context<'_>) -> Self::Return {
        ctx.set_position(self.location);

        match &self.data {
            TypeKind::Arrow(arrow) => {
                let left = arrow.left.infer(ctx.clone());
                let right = arrow.right.infer(ctx);
                left.arrow(right)
            }

            TypeKind::Variable(v) => {
                if ctx.lookup_type(&v.name).is_some() {
                    MonoType::typ(v.name.to_owned())
                } else if ctx.typ_map.contains(&v.name) {
                    MonoType::var(v.name.to_owned())
                } else {
                    ctx.new_error(format!("unbound type variable '{}'", v.name))
                }
            }

            TypeKind::Tuple(tuple) => Rc::new(MonoType::Tuple(
                tuple
                    .types
                    .iter()
                    .map(|typ| typ.infer(ctx.clone()))
                    .collect(),
            )),

            // TODO: Error when try to infer a forall inside other types.
            TypeKind::Forall(forall) => {
                TypeScheme {
                    names: forall.args.clone(),
                    mono: forall.body.infer(ctx.extend_types(&forall.args)),
                }
                .instantiate(ctx)
                .0
            }

            TypeKind::Application(app) => match ctx.lookup_type(&app.fun) {
                Some(sig) if sig.params.len() == app.args.len() => Rc::new(MonoType::Application(
                    sig.name,
                    app.args.iter().map(|typ| typ.infer(ctx.clone())).collect(),
                )),

                Some(sig) => ctx.new_error(format!(
                    "expected {} arguments but got {} in type",
                    sig.params.len(),
                    app.args.len()
                )),

                None => ctx.new_error(format!("unbound variable '{}'", app.fun)),
            },
        }
    }
}
