use crate::context::Ctx;
use crate::{check::Check, infer::Infer, types::*};
use atiny_tree::{elaborated::FnBody, r#abstract::*, SeqIter};
use itertools::Itertools;
use std::iter::FromIterator;
use std::{collections::HashSet, iter, rc::Rc};

use super::module::ModuleMap;

impl Infer for Vec<TopLevel> {
    type Context<'a> = &'a mut Ctx;
    type Return = ModuleMap;

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let mut top_levels = SeqIter::new(self);

        let cons = ctx.infer_all(top_levels.as_iter::<TypeDecl>());
        let mut module_map = ctx.resolve_imports(&mut top_levels);
        let bodies = ctx.infer_all(top_levels.as_iter::<FnDecl>());

        module_map.push((ctx.id, (cons, bodies)));
        module_map
    }
}

impl Ctx {
    pub fn resolve_imports(&mut self, iter: impl IntoIterator<Item = UseDecl>) -> ModuleMap {
        let mut infered = ModuleMap::default();

        for UseDecl(Qualifier(quali), item) in iter.into_iter() {
            let file = {
                let mut prog = self.program.borrow_mut();
                let path = quali.into_iter().join(".");

                prog.file_system
                    .get_file_relative(&path, self.id)
                    .expect("IO Error")
            };

            self.program.return_ctx(self.clone());

            self.program
                .clone()
                .get_module(file, |ctx, parsed: Option<Vec<_>>| {
                    if let Some(ModuleMap(module)) = parsed.infer(ctx) {
                        infered.extend(module);
                    }

                    self.register_imports(ctx, item.clone());
                });
        }

        infered
    }

    pub fn infer_all<T, O>(&mut self, iter: impl IntoIterator<Item = T>) -> O
    where
        T: for<'a> Infer<Context<'a> = &'a mut Self>,
        O: FromIterator<T::Return>,
    {
        iter.into_iter().map(|item| item.infer(self)).collect()
    }
}

impl Infer for TypeDecl {
    type Context<'a> = &'a mut Ctx;
    type Return = (String, TypeDeclKind);

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (value, names) = match self.constructors {
            TypeDeclKind::Sum(ref cons) => (
                TypeValue::Sum(Vec::new()),
                cons.iter().map(|c| c.name.clone()).collect(),
            ),
            TypeDeclKind::Product(_) => (TypeValue::Product(Vec::new()), Vec::new()),
        };

        let type_sig = TypeSignature {
            name: self.name.clone(),
            params: self.params.clone(),
            value,
            names,
        };

        ctx.signatures.types.insert(self.name.clone(), type_sig);

        (self.name, self.constructors)
    }
}

impl Infer for (String, TypeDeclKind) {
    type Context<'a> = &'a mut Ctx;
    type Return = ();

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (decl_name, kind) = self;
        let name = iter::repeat(decl_name.as_str());

        match kind {
            TypeDeclKind::Sum(constrs) => name.zip(constrs).for_each(|constr| constr.infer(ctx)),
            TypeDeclKind::Product(fields) => name.zip(fields).for_each(|field| field.infer(ctx)),
        }
    }
}

impl Infer for (&str, Field) {
    type Context<'a> = &'a mut Ctx;
    type Return = ();

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (decl_name, field) = self;

        let Some(TypeSignature { params, .. }) = &ctx.lookup_type(decl_name) else {
            panic!("The String should be a valid type signature name on the Ctx");
        };

        let new_ctx = ctx.extend_types(params);

        let mono = field.ty.infer(new_ctx);

        let sig_value = &mut ctx.signatures.types.get_mut(decl_name).unwrap().value;

        if let TypeValue::Product(ref mut rec) = sig_value {
            rec.push((field.name.clone(), mono));
        }

        ctx.signatures
            .fields
            .entry(field.name)
            .or_default()
            .insert(decl_name.to_string());
    }
}

impl Infer for (&str, Constructor) {
    type Context<'a> = &'a mut Ctx;
    type Return = ();

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (decl_name, constr) = self;

        let Some(TypeSignature { params, .. }) = &ctx.lookup_type(decl_name) else {
            panic!("The String should be a valid type signature name on the Ctx");
        };

        let application = Rc::new(MonoType::Application(
            decl_name.to_string(),
            params.iter().cloned().map(MonoType::var).collect(),
        ));

        let new_ctx = ctx.extend_types(params);

        let args: Vec<_> = constr
            .types
            .iter()
            .map(|t| t.infer(new_ctx.clone()))
            .collect();

        let value = Rc::new(ConstructorSignature::new(
            constr.name.clone(),
            params.clone(),
            MonoType::rfold_arrow(args.iter().cloned(), application),
            args,
        ));

        let sig_value = &mut ctx.signatures.types.get_mut(decl_name).unwrap().value;

        if let TypeValue::Sum(ref mut sum) = sig_value {
            sum.push(value.clone());
        }

        ctx.signatures
            .values
            .insert(constr.name, DeclSignature::Constructor(value));
    }
}

impl Infer for FnDecl {
    type Context<'a> = &'a mut Ctx;
    type Return = (String, Expr);

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let mut set = HashSet::new();

        for (_, ty) in &self.params {
            ctx.free_variables(ty, &mut set);
        }

        let new_ctx = ctx.extend_types(set.iter());

        let args: Vec<_> = self
            .params
            .iter()
            .map(|(a, p)| (a.to_owned(), p.infer(new_ctx.clone())))
            .collect();

        let ret = self.ret.infer(new_ctx);

        let entire_type = TypeScheme::new(
            set.into_iter().collect(),
            MonoType::rfold_arrow(args.iter().map(|(_, ty)| ty.clone()), ret),
        );

        let sig = DeclSignature::Function(FunctionSignature {
            name: self.name.clone(),
            args,
            entire_type,
        });

        ctx.signatures.values.insert(self.name.clone(), sig);

        (self.name, self.body)
    }
}

impl Infer for (String, Expr) {
    type Context<'a> = &'a mut Ctx;
    type Return = FnBody<Type>;

    fn infer(self, ctx: Self::Context<'_>) -> Self::Return {
        let (fn_name, body) = self;

        let Some(DeclSignature::Function(sig)) = ctx.signatures.values.get(&fn_name) else {
            panic!("The String should be a valid function signature name on the Ctx");
        };

        let mut new_ctx = ctx.extend_types(sig.type_variables());

        for (arg_pat, arg_type) in &sig.args {
            let witness = new_ctx.single_exhaustiveness(arg_pat, arg_type.clone());

            if let Err(err) = witness.result() {
                new_ctx.set_position(arg_pat.location);
                new_ctx.error(format!(
                    "refutable pattern in function argument. pattern `{}` not covered",
                    err
                ));
            };
        }

        FnBody(body.check(new_ctx, sig.return_type()))
    }
}

impl Ctx {
    fn free_variables(&self, ty: &TypeNode, set: &mut HashSet<String>) {
        match &ty.data {
            TypeKind::Arrow(arrow) => {
                self.free_variables(&arrow.left, set);
                self.free_variables(&arrow.right, set);
            }
            TypeKind::Variable(v) => {
                if !self.typ_map.contains(&v.name) {
                    set.insert(v.name.clone());
                }
            }
            TypeKind::Forall(forall) => {
                self.free_variables(&forall.body, set);
                for arg in &forall.args {
                    set.remove(arg);
                }
            }
            TypeKind::Application(app) => {
                for arg in &app.args {
                    self.free_variables(arg, set);
                }
            }
            TypeKind::Tuple(tuple) => {
                for arg in &tuple.types {
                    self.free_variables(arg, set);
                }
            }
        }
    }
}
