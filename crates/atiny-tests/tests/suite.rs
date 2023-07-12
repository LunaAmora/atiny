#![feature(custom_test_frameworks)]
#![test_runner(atiny_tests::test_runner)]

use atiny_checker::infer::Infer;
use atiny_checker::program::Program;
use atiny_tree::r#abstract::Expr;
use itertools::Itertools;

#[macro_use]
extern crate atiny_tests;

mk_test!("/suite/expr/", |file_name| {
    let program = Program::new(file_name).expect("IO error");

    let mut parsed_result = String::new();
    program.get_entry(|ctx, parsed: Option<Expr>| {
        if let Some((t, _)) = parsed.as_ref().infer(ctx.clone()) {
            parsed_result = t.to_string();
        }
    });

    program
        .take_errors()
        .map_or_else(|| parsed_result, |errs| errs.into_iter().join(""))
});

mk_test!("/suite/parsing/", |file_name| {
    use itertools::Itertools;
    let program = Program::new(file_name).expect("IO error");

    let mut parsed_result = String::new();

    program.get_entry(|_, parsed: Option<Vec<_>>| {
        parsed_result = parsed
            .iter()
            .flatten()
            .map(|top| top.to_string())
            .join("\n");
    });

    program
        .take_errors()
        .map_or_else(|| parsed_result, |errs| errs.into_iter().join(""))
});

mk_test!("/suite/", |file_name| {
    let program = Program::new(file_name).expect("IO error");

    let _ = program
        .get_entry(|ctx, parsed: Option<Vec<_>>| parsed.infer(ctx))
        .infer(program.clone());

    program
        .take_errors()
        .map_or_else(String::new, |err| err.into_iter().join(""))
});
