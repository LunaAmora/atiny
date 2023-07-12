//! This module is useful for type checking [Expr] and [TopLevel] definitions using the
//! hindley-milner type system extended with type classes and linear types.

pub mod context;
pub mod program;
pub mod types;
pub mod unify;

pub mod check;
pub mod infer;

pub mod exhaustive;
