//! This crate is useful to the compilation of elaborated trees to MIR. It is not intended to be used

pub mod builder;
pub mod compile;

use std::fmt::Display;

/// The type of a node in the IR.
pub enum Type {
    Ptr,
}

/// A parameter of a function.
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

/// A function with a bunch of basic blocks
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub body: Vec<BasicBlock>,
}

/// A label for a basic block
pub struct Label(String);

/// A variable name that is used to refer to a value
pub enum Variable {
    Named(String),
    Number(usize),
}

/// A reference to a value or a value itself
pub enum ValueRef {
    Variable(Variable),
    Constant(i32),
}

/// A basic block with a bunch of instructions and a terminator
pub struct BasicBlock {
    pub name: Label,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

/// The terminator of a basic block
pub enum Terminator {
    Branch(Label),
    BranchCond(Variable, Label, Label),
    Return(Variable),
}

/// A instruction that is part of a basic block and that executes some code.
pub enum Instruction {
    Assign(Variable, ValueRef),
    Load(Variable, ValueRef),
    Store(Variable, ValueRef),
    Alloc(Variable),
    GetElement(Variable, ValueRef, ValueRef),
    Phi(Variable, Vec<(Variable, Label)>),
}

impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ptr => write!(f, "ptr"),
        }
    }
}

impl Display for Parameter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} : {}", self.name, self.ty)
    }
}

impl Display for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parameters = self
            .parameters
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(f, "fn {}({}) {{", self.name, parameters)?;
        for bb in &self.body {
            write!(f, "{}", bb)?;
        }
        write!(f, "}}")
    }
}

impl Display for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "    {}:", self.name)?;
        for inst in &self.instructions {
            writeln!(f, "        {}", inst)?;
        }
        writeln!(f, "        {}", self.terminator)
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, ".{}", self.0)
    }
}

impl Display for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{}", name),
            Self::Number(n) => write!(f, "%{}", n),
        }
    }
}

impl Display for ValueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Variable(v) => write!(f, "{}", v),
            Self::Constant(c) => write!(f, "{}", c),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Assign(v, r) => write!(f, "{} = {}", v, r),
            Self::Load(v, r) => write!(f, "{} = load {}", v, r),
            Self::Store(v, r) => write!(f, "store {} {}", v, r),
            Self::Alloc(v) => write!(f, "{} = alloc", v),
            Self::GetElement(v, r1, r2) => write!(f, "{} = gep {} {}", v, r1, r2),
            Self::Phi(v, l) => {
                let l = l
                    .iter()
                    .map(|(v, l)| format!("{} {}", v, l))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{} = phi {}", v, l)
            }
        }
    }
}

impl Display for Terminator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Branch(l) => write!(f, "branch {}", l),
            Self::BranchCond(v, l1, l2) => write!(f, "branch-cond {} {} {}", v, l1, l2),
            Self::Return(v) => write!(f, "return {}", v),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{BasicBlock, Function, Instruction, Label};

    #[test]
    pub fn test_print() {
        let function = Function {
            name: "main".to_string(),
            parameters: vec![],
            body: vec![BasicBlock {
                name: Label("entry".to_string()),
                instructions: vec![
                    Instruction::Assign(
                        crate::Variable::Named("a".to_string()),
                        crate::ValueRef::Constant(42),
                    ),
                    Instruction::GetElement(
                        crate::Variable::Named("b".to_string()),
                        crate::ValueRef::Variable(crate::Variable::Named("a".to_string())),
                        crate::ValueRef::Constant(1),
                    ),
                ],
                terminator: crate::Terminator::Branch(Label("exit".to_string())),
            }],
        };

        println!("{}", function);
    }
}
