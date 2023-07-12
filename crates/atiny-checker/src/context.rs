//! The context is primarily a list of bindings from variable names to type that is on the left side
//! of a type judgment.

use std::{cell::RefCell, fmt::Display, rc::Rc};

use atiny_error::{Error, Message, SugestionKind};
use atiny_location::{ByteRange, Located};
use atiny_parser::io::NodeId;
use atiny_tree::r#abstract::TypeDecl;
use itertools::Itertools;

use crate::{infer::Infer, program::Program};

use super::types::*;

#[derive(Clone, Default, Debug)]
pub struct Signatures {
    pub types: im_rc::OrdMap<String, TypeSignature>,
    pub values: im_rc::OrdMap<String, DeclSignature>,
    pub fields: im_rc::OrdMap<String, im_rc::OrdSet<String>>,
}

impl Display for Signatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Types:\n    {}", self.types.values().join("\n    "))?;
        writeln!(f, "\nValues:\n    {}", self.values.values().join("\n    "))
    }
}

#[derive(Clone, Debug)]
pub enum Imports {
    Star,
    Module,
    Items(Vec<String>),
}

#[derive(Clone)]
pub struct Ctx {
    pub id: NodeId,
    pub program: Program,
    pub imports: Rc<RefCell<im_rc::HashMap<NodeId, Imports>>>,
    counter: Rc<RefCell<usize>>,
    pub errors: Rc<RefCell<Vec<Error>>>,
    pub map: im_rc::OrdMap<String, Rc<TypeScheme>>,
    pub typ_map: im_rc::OrdSet<String>,
    pub location: ByteRange,
    pub level: usize,
    pub signatures: Signatures,
}

impl Ctx {
    pub fn new(id: NodeId, program: Program) -> Self {
        let mut ctx = Self {
            id,
            program,
            imports: Default::default(),
            counter: Default::default(),
            errors: Default::default(),
            map: Default::default(),
            typ_map: Default::default(),
            location: Default::default(),
            level: Default::default(),
            signatures: Default::default(),
        };

        ctx.signatures.types.insert(
            "Int".to_string(),
            TypeSignature::new_opaque("Int".to_string()),
        );

        let int = MonoType::typ("Int".to_string());
        let sig = MonoType::arrow(int.clone(), MonoType::arrow(int.clone(), int)).to_poly();

        ctx.map.extend([
            ("add".to_string(), sig.clone()),
            ("sub".to_string(), sig.clone()),
            ("mul".to_string(), sig.clone()),
            ("div".to_string(), sig),
        ]);

        #[allow(clippy::let_unit_value)]
        {
            let cons = TypeDecl::unit().infer(&mut ctx);
            let _ = cons.infer(&mut ctx);
        }

        ctx
    }
}

impl Ctx {
    // Extends a context with a new binding.
    pub fn extend(&self, name: String, typ: Rc<TypeScheme>) -> Self {
        Self {
            map: self.map.update(name, typ),
            ..self.clone()
        }
    }

    /// Extends a context with a type name (for type variables).
    pub fn extend_type(&self, name: String) -> Self {
        Self {
            typ_map: self.typ_map.update(name),
            ..self.clone()
        }
    }

    /// Extends a context with a list of type names (for type variables).
    pub fn extend_types<'a, N: IntoIterator<Item = &'a String>>(&self, names: N) -> Self {
        Self {
            typ_map: self.typ_map.clone().union(names.into_iter().collect()),
            ..self.clone()
        }
    }

    /// Increase the level of the context, so it measures where we are inside the forall bindings.
    /// It only increases during let values because let values are always generalized so it counts
    /// as if we were inside a forall.
    pub fn level_up(&self) -> Self {
        Self {
            level: self.level + 1,
            ..self.clone()
        }
    }

    /// Sets the current location that we are type checking inside of the context.
    pub fn set_position(&mut self, location: ByteRange) {
        self.location = location;
    }

    /// Creates a new name for a type variable.
    pub fn new_name(&self) -> String {
        let counter = {
            let mut counter = self.counter.borrow_mut();
            *counter += 1;
            *counter
        };

        format!("'{}", (97 + ((counter - 1) % 26)) as u8 as char)
    }

    /// Looks up a type variable name in the context.
    pub fn lookup(&self, name: &str) -> Option<Rc<TypeScheme>> {
        self.map
            .get(name)
            .cloned()
            .or_else(|| {
                self.signatures.values.get(name).map(|decl| match decl {
                    DeclSignature::Function(fun) => fun.entire_type.clone(),
                    DeclSignature::Constructor(decl) => decl.typ.clone(),
                })
            })
            .or_else(|| self.lookup_on_imports(name, |ctx| ctx.lookup(name)))
    }

    pub fn lookup_cons(&self, name: &str) -> Option<Rc<ConstructorSignature>> {
        self.signatures
            .values
            .get(name)
            .and_then(|decl| match decl {
                DeclSignature::Function(_) => None,
                DeclSignature::Constructor(cons) => Some(cons.clone()),
            })
            .or_else(|| self.lookup_on_imports(name, |ctx| ctx.lookup_cons(name)))
    }

    pub fn lookup_type(&self, name: &str) -> Option<TypeSignature> {
        self.signatures
            .types
            .get(name)
            .cloned()
            .or_else(|| self.lookup_on_imports(name, |ctx| ctx.lookup_type(name)))
    }

    fn lookup_on_imports<T>(&self, name: &str, lookup: impl Fn(&Self) -> Option<T>) -> Option<T> {
        for (id, import) in self.imports.borrow().iter() {
            match import {
                Imports::Items(items) => {
                    if items.iter().any(|i| name.eq(i.as_str())) {
                        return lookup(&self.get_ctx_by_id(id));
                    };
                }
                Imports::Star => return lookup(&self.get_ctx_by_id(id)),
                Imports::Module => {}
            }
        }
        None
    }

    fn get_ctx_by_id(&self, id: &NodeId) -> Self {
        self.program.borrow().modules[id]
            .clone()
            .expect("ICE: context was not stored in the program")
    }

    pub fn error(&self, msg: String) {
        self.errors
            .borrow_mut()
            .push(Error::new(Message::Single(msg), self.location));
    }

    pub fn errors(&self, msg: Vec<String>) {
        self.errors
            .borrow_mut()
            .push(Error::new(Message::Multi(msg), self.location));
    }

    pub fn suggestion(&self, msg: String, sugestion_kind: SugestionKind) {
        self.errors.borrow_mut().push(Error::new_sugestion(
            Message::Single(msg),
            sugestion_kind,
            self.location,
        ));
    }

    pub fn suggestions(&self, msg: Vec<String>, sugestion_kind: SugestionKind) {
        self.errors.borrow_mut().push(Error::new_sugestion(
            Message::Multi(msg),
            sugestion_kind,
            self.location,
        ));
    }

    pub fn dyn_error(&self, msg: impl Display + 'static) {
        self.errors
            .borrow_mut()
            .push(Error::new_dyn(msg, self.location));
    }

    pub fn take_errors(&self) -> Vec<Error> {
        std::mem::take(&mut self.errors.borrow_mut())
    }

    pub fn err_count(&self) -> usize {
        self.errors.borrow().len()
    }

    /// Creates a new hole type.
    pub fn new_hole(&self) -> Type {
        MonoType::new_hole(self.new_name(), self.level)
    }

    pub fn register_imports(&mut self, ctx: &Self, item: Option<Located<String>>) {
        let imports = match item {
            None => Imports::Module,

            Some(item) if item.data.eq("*") => Imports::Star,

            Some(item) if item.data.starts_with(|c: char| c.is_ascii_uppercase()) => {
                let mut names = Vec::new();

                match ctx.lookup_type(&item.data) {
                    Some(sig) => {
                        names.push(sig.name);
                        for name in sig.names {
                            names.push(name);
                        }
                    }

                    None => {
                        self.set_position(item.location);
                        self.error(format!("could not find Type `{}` import", item));
                        return;
                    }
                }

                Imports::Items(names)
            }

            Some(item) => Imports::Items(vec![item.data]),
        };

        self.update_imports(ctx.id, imports);
    }

    pub fn update_imports(&self, id: NodeId, update: Imports) {
        let mut imports = self.imports.borrow_mut();
        use Imports::*;

        match imports.get_mut(&id) {
            None => _ = imports.insert(id, update),

            Some(current) => match (current, update) {
                (Star, Star) => {}
                (Star, Items(_)) => {}
                (Module, Module) => {}

                (Star, Module) => todo!(),
                (Module, Star) => todo!(),
                (Module, Items(_)) => todo!(),
                (Items(_), Module) => todo!(),

                (Items(_), Star) => _ = imports.insert(id, Star),
                (Items(ref mut items), Items(names)) => items.extend(names),
            },
        }
    }
}

pub trait InferError<T> {
    fn new_error(&self, msg: String) -> T;
}

impl InferError<Type> for Ctx {
    fn new_error(&self, msg: String) -> Type {
        self.error(msg);
        Rc::new(MonoType::Error)
    }
}
