use std::io;
use std::mem::take;
use std::ops::Deref;
use std::{cell::RefCell, collections::HashMap, fs::read_to_string, path::PathBuf, rc::Rc};

use atiny_error::Error;
use atiny_parser::io::{File, FileSystem, NodeId, VirtualFileSystem};
use atiny_parser::{Parser, Parsers};
use atiny_tree::elaborated::FnBody;

use crate::context::Ctx;
use crate::types::Type;

#[derive(Clone)]
pub struct Program(Rc<RefCell<Prog>>);

impl Deref for Program {
    type Target = Rc<RefCell<Prog>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Program {
    pub fn new(file: PathBuf) -> io::Result<Self> {
        FileSystem::new_with(&file, |file| read_to_string(file).map(Rc::from))
            .map(|(file_system, entry_point)| Prog {
                file_system: Box::new(file_system),
                entry_point,
                modules: HashMap::new(),
                elaborated: HashMap::new(),
                parser: Parsers::default(),
            })
            .map(|prog| Self(Rc::new(RefCell::new(prog))))
    }

    pub fn print_errors(&self) {
        self.borrow_mut().print_errors();
    }

    pub fn take_errors(&self) -> Option<Vec<String>> {
        self.borrow_mut().take_errors()
    }

    pub fn return_ctx(&self, ctx: Ctx) {
        let prog = &mut self.borrow_mut();
        prog.modules.insert(ctx.id, Some(ctx));
    }

    pub fn get_entry<Out, R>(&self, f: impl FnMut(&mut Ctx, Option<Out>) -> R) -> R
    where
        Parsers: Parser<Out>,
    {
        let entry_point = self.borrow().get_entry();
        self.get_module(entry_point, f)
    }

    pub fn get_module<Out, R>(&self, file: File, mut f: impl FnMut(&mut Ctx, Option<Out>) -> R) -> R
    where
        Parsers: Parser<Out>,
    {
        let (mut ctx, parsed) = self.take_or_parse(file);
        let result = f(&mut ctx, parsed);
        self.return_ctx(ctx);
        result
    }

    fn take_or_parse<Out>(&self, file: File) -> (Ctx, Option<Out>)
    where
        Parsers: Parser<Out>,
    {
        if let Some(ctx) = { self.borrow_mut().take_ctx(file.id) } {
            return (ctx, None);
        }

        self.parse_file_as_ctx(file)
    }

    fn parse_file_as_ctx<Out>(&self, file: File) -> (Ctx, Option<Out>)
    where
        Parsers: Parser<Out>,
    {
        let parsed = { self.borrow_mut().parse_file(&file) };
        let ctx = Ctx::new(file.id, self.clone());

        let top = parsed.map_or_else(
            |err| {
                ctx.errors.borrow_mut().push(err);
                None
            },
            Some,
        );

        (ctx, top)
    }
}

pub struct Prog {
    pub file_system: Box<dyn VirtualFileSystem<PathBuf, io::Error>>,
    pub entry_point: NodeId,
    pub modules: HashMap<NodeId, Option<Ctx>>,
    pub elaborated: HashMap<NodeId, Vec<FnBody<Type>>>,
    pub parser: Parsers,
}

impl Prog {
    pub fn get_entry(&self) -> File {
        self.file_system.get_file(self.entry_point).unwrap()
    }

    pub fn take_ctx(&mut self, id: NodeId) -> Option<Ctx> {
        match self.modules.get_mut(&id) {
            Some(None) => panic!("ICE: context was already taken"),
            Some(ctx) => take(ctx),
            None => None,
        }
    }

    fn parse_file<Out>(&mut self, file: &File) -> Result<Out, Error>
    where
        Parsers: Parser<Out>,
    {
        let None = self.modules.insert(file.id, None) else {
            panic!("ICE: attempted to parse the same file twice")
        };

        self.parser.parse(&file.code)
    }

    pub fn print_errors(&mut self) {
        if let Some(errs) = self.take_errors() {
            for err in errs {
                eprint!("{}", err);
            }
        }
    }

    pub fn take_errors(&mut self) -> Option<Vec<String>> {
        let result: Vec<String> = self
            .modules
            .values_mut()
            .map(|module| module.as_ref().expect("ICE: module ctx was missing"))
            .flat_map(|ctx| {
                ctx.take_errors()
                    .into_iter()
                    .map(|err| {
                        let file = self.file_system.get_file(ctx.id).unwrap();
                        let name = self.file_system.get_node_full_name(ctx.id);
                        err.with_code(&file.code, &name).to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}
