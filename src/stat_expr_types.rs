use crate::name::VarName;
use crate::num::Numeral;
use crate::op::{BinaryOp, UnaryOp};

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionCall<'a> {
    pub method: VarName<'a>,
    pub pos_args: Vec<Exp<'a>>,
    pub dict_args: Vec<(VarName<'a>, Exp<'a>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefCall<'a> {
    pub name: VarName<'a>,
    pub arg: Exp<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition<'a> {
    pub cond: Exp<'a>,
    pub exp1: Exp<'a>,
    pub exp2: Exp<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Exp<'a> {
    Na,
    Bool(bool),
    Num(Numeral),
    Str(&'a str),
    Color(&'a str),
    VarName(VarName<'a>),
    RetTuple(Box<Vec<VarName<'a>>>),
    Tuple(Box<Vec<Exp<'a>>>),
    FuncCall(Box<FunctionCall<'a>>),
    RefCall(Box<RefCall<'a>>),
    Condition(Box<Condition<'a>>),
    Ite(Box<IfThenElse<'a>>),
    ForRange(Box<ForRange<'a>>),
    UnaryExp(UnaryOp, Box<Exp<'a>>),
    BinaryExp(BinaryOp, Box<Exp<'a>>, Box<Exp<'a>>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OpOrExp2<'a> {
    Op(UnOrBinOp),
    Exp2(Exp2<'a>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnOrBinOp {
    UnaryOp(UnaryOp),
    BinaryOp(BinaryOp),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlatExp<'a>(pub Vec<OpOrExp2<'a>>);

#[derive(Clone, Debug, PartialEq)]
pub enum Exp2<'a> {
    Na,
    Bool(bool),
    Num(Numeral),
    Str(&'a str),
    Color(&'a str),
    VarName(VarName<'a>),
    RetTuple(Box<Vec<VarName<'a>>>),
    Tuple(Box<Vec<Exp<'a>>>),
    FuncCall(Box<FunctionCall<'a>>),
    RefCall(Box<RefCall<'a>>),
    Condition(Box<Condition<'a>>),
    Ite(Box<IfThenElse<'a>>),
    ForRange(Box<ForRange<'a>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assignment<'a> {
    pub vars: Vec<VarName<'a>>,
    pub vals: Vec<Exp<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block<'a> {
    pub stmts: Vec<Statement<'a>>,
    pub ret_stmt: Option<Exp<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IfThenElse<'a> {
    pub cond: Exp<'a>,
    pub then_blk: Block<'a>,
    pub elseifs: Vec<(Exp<'a>, Block<'a>)>,
    pub else_blk: Option<Block<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForRange<'a> {
    pub var: VarName<'a>,
    pub start: Exp<'a>,
    pub end: Exp<'a>,
    pub step: Option<Exp<'a>>,
    pub do_blk: Block<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDef<'a> {
    pub name: VarName<'a>,
    pub params: Vec<VarName<'a>>,
    pub body: Block<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement<'a> {
    Break,
    Continue,
    Assignment(VarName<'a>, Box<Exp<'a>>),
    Ite(Box<IfThenElse<'a>>),
    ForRange(Box<ForRange<'a>>),
    FuncCall(Box<FunctionDef<'a>>),
}
