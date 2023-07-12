use crate::infer::Infer;
use crate::program::Program;
use atiny_parser::io::NodeId;
use atiny_tree::r#abstract::*;
use std::ops::{Deref, DerefMut};

pub type ModuleBodies = (Vec<(String, TypeDeclKind)>, Vec<(String, Expr)>);

#[derive(Default)]
pub struct ModuleMap(pub Vec<(NodeId, ModuleBodies)>);

impl Deref for ModuleMap {
    type Target = Vec<(NodeId, ModuleBodies)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ModuleMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Infer for ModuleMap {
    type Context<'a> = Program;
    type Return = ();

    fn infer(self, program: Self::Context<'_>) -> Self::Return {
        let mut bodies = Vec::new();
        for (id, (type_bodies, fn_bodies)) in self.0 {
            bodies.push((id, fn_bodies));

            let mut ctx = {
                let mut prog = program.borrow_mut();
                prog.take_ctx(id).unwrap()
            };

            let _: () = ctx.infer_all(type_bodies);

            program.return_ctx(ctx);
        }

        for (id, fn_bodies) in bodies {
            let mut ctx = {
                let mut prog = program.borrow_mut();
                prog.take_ctx(id).unwrap()
            };

            let bodies = ctx.infer_all(fn_bodies);

            let mut prog = program.borrow_mut();
            prog.elaborated.insert(ctx.id, bodies);
            prog.modules.insert(ctx.id, Some(ctx));
        }
    }
}
