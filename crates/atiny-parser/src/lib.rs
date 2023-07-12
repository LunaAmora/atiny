//! This module parses a text source code into a [atiny_tree::abstract] tree. Following the spec
//! that is described in the README.md and using a LALR(1) parser generator called LALRPOP. It does
//! not include error recovery strategies nor incremental parsing.

#![feature(trait_alias)]
use atiny_error::Error;
use atiny_tree::r#abstract::{Expr, TopLevel};
use error::from_lalrpop;

pub mod error;
pub mod io;

#[macro_use]
extern crate lalrpop_util;

lalrpop_mod!(
    #[allow(warnings)]
    /// The parsing module
    parser
);

pub trait Parser<T>
where
    Parsers: Parser<T>,
{
    fn parse(&self, input: &str) -> Result<T, Error>;
}

impl Parser<Vec<TopLevel>> for ProgramParser {
    fn parse(&self, input: &str) -> Result<Vec<TopLevel>, Error> {
        self.parse(input).map_err(from_lalrpop)
    }
}

impl Parser<Expr> for ExprParser {
    fn parse(&self, input: &str) -> Result<Expr, Error> {
        self.parse(input).map_err(from_lalrpop)
    }
}

pub struct Parsers {
    pub top_level: Box<dyn Parser<Vec<TopLevel>>>,
    pub expr: Box<dyn Parser<Expr>>,
}

impl Parser<Vec<TopLevel>> for Parsers {
    fn parse(&self, input: &str) -> Result<Vec<TopLevel>, Error> {
        self.top_level.parse(input)
    }
}

impl Parser<Expr> for Parsers {
    fn parse(&self, input: &str) -> Result<Expr, Error> {
        self.expr.parse(input)
    }
}

impl Default for Parsers {
    fn default() -> Self {
        Self {
            top_level: Box::new(ProgramParser::new()),
            expr: Box::new(ExprParser::new()),
        }
    }
}

pub use parser::*;
