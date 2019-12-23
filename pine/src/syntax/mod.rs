use crate::ast::error::PineErrorKind;
use crate::ast::input::StrRange;
use crate::ast::name::VarName;
use crate::ast::num::Numeral;
use crate::ast::op::{BinaryOp, UnaryOp};
use crate::ast::stat_expr_types::{
    Assignment, BinaryExp, Block, Condition, DataType, Exp, ForRange, FunctionCall, FunctionDef,
    IfThenElse, PrefixExp, RefCall, Statement, TupleNode, TypeCast, UnaryExp, VarAssignment,
};
use crate::ast::state::PineInputError;
use crate::ast::syntax_type::{FunctionType, SimpleSyntaxType, SyntaxType};
use std::collections::HashMap;
use std::rc::Rc;

mod convert;
pub mod ctxid_parser;
mod type_cast;

use convert::{common_type, implicity_convert, similar_type, simple_to_series};
use type_cast::{explicity_type_cast, implicity_type_cast};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ContextType {
    Normal,
    IfElseBlock,
    ForRangeBlock,
    FuncDefBlock,
}

pub trait SyntaxCtx<'a> {
    fn declare_var(&mut self, name: &'a str, t: SyntaxType<'a>);

    fn update_var(&mut self, name: &'a str, t: SyntaxType<'a>);

    fn get_var(&self, name: &'a str) -> Option<&SyntaxType<'a>>;

    fn get_var_scope(&self, name: &'a str) -> Option<&SyntaxType<'a>>;
}

pub struct SyntaxContext<'a> {
    parent: Option<*mut (dyn SyntaxCtx<'a> + 'a)>,
    context_type: ContextType,

    vars: HashMap<&'a str, SyntaxType<'a>>,
}

pub fn downcast_ctx<'a>(item: *mut (dyn SyntaxCtx<'a> + 'a)) -> &mut SyntaxContext<'a> {
    unsafe {
        let raw: *mut dyn SyntaxCtx<'a> = item;
        let t = raw as *mut SyntaxContext<'a>;
        t.as_mut().unwrap()
    }
}

impl<'a> SyntaxCtx<'a> for SyntaxContext<'a> {
    fn declare_var(&mut self, name: &'a str, t: SyntaxType<'a>) {
        self.vars.insert(name, t);
    }

    fn update_var(&mut self, name: &'a str, t: SyntaxType<'a>) {
        if self.vars.contains_key(name) {
            self.vars.insert(name, t);
        } else if let Some(p) = self.parent {
            downcast_ctx(p).update_var(name, t);
        } else {
            unreachable!();
        }
    }

    fn get_var(&self, name: &'a str) -> Option<&SyntaxType<'a>> {
        if self.vars.contains_key(name) {
            self.vars.get(name)
        } else if let Some(p) = self.parent {
            downcast_ctx(p).get_var(name)
        } else {
            None
        }
    }

    fn get_var_scope(&self, name: &'a str) -> Option<&SyntaxType<'a>> {
        self.vars.get(name)
    }
}

impl<'a> SyntaxContext<'a> {
    pub fn new(
        parent: Option<*mut (dyn SyntaxCtx<'a> + 'a)>,
        context_type: ContextType,
    ) -> SyntaxContext<'a> {
        SyntaxContext {
            parent,
            context_type,
            vars: HashMap::new(),
        }
    }

    pub fn get_type(&self) -> ContextType {
        self.context_type
    }
}

pub struct SyntaxParser<'a> {
    ctxs: HashMap<i32, Box<dyn SyntaxCtx<'a> + 'a>>,
    context: *mut (dyn SyntaxCtx<'a> + 'a),
    user_funcs: HashMap<&'a str, *mut FunctionDef<'a>>,
    errors: Vec<PineInputError>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseValue<'a> {
    pub syntax_type: SyntaxType<'a>,
    pub varname: Option<&'a str>,
}

impl<'a> ParseValue<'a> {
    pub fn new(syntax_type: SyntaxType<'a>, varname: &'a str) -> ParseValue<'a> {
        ParseValue {
            syntax_type,
            varname: Some(varname),
        }
    }
    pub fn new_with_type(syntax_type: SyntaxType<'a>) -> ParseValue<'a> {
        ParseValue {
            syntax_type,
            varname: None,
        }
    }
}

type ParseResult<'a> = Result<ParseValue<'a>, PineInputError>;

impl<'a> SyntaxParser<'a> {
    pub fn new() -> SyntaxParser<'a> {
        let mut ctxs: HashMap<i32, Box<dyn SyntaxCtx<'a> + 'a>> = HashMap::new();
        let top_ctx: Box<dyn SyntaxCtx<'a> + 'a> =
            Box::new(SyntaxContext::new(None, ContextType::Normal));
        ctxs.insert(0, top_ctx);
        SyntaxParser {
            context: &mut **ctxs.get_mut(&0).unwrap(),
            ctxs,
            user_funcs: HashMap::new(),
            errors: vec![],
        }
    }

    pub fn new_with_vars(vars: HashMap<&'a str, SyntaxType<'a>>) -> SyntaxParser<'a> {
        let mut ctxs: HashMap<i32, Box<dyn SyntaxCtx<'a> + 'a>> = HashMap::new();
        let mut context = SyntaxContext::new(None, ContextType::Normal);
        context.vars = vars;
        ctxs.insert(0, Box::new(context));
        SyntaxParser {
            context: &mut **ctxs.get_mut(&0).unwrap(),
            ctxs,
            user_funcs: HashMap::new(),
            errors: vec![],
        }
    }

    pub fn catch(&mut self, err: PineInputError) {
        self.errors.push(err)
    }

    fn parse_std_func_call(
        &mut self,
        func_call: &mut FunctionCall<'a>,
        fun_type: &Rc<FunctionType<'a>>,
    ) -> ParseResult<'a> {
        let mut pos_arg_type = vec![];
        for arg in func_call.pos_args.iter_mut() {
            pos_arg_type.push(self.parse_exp(arg)?);
        }
        let mut dict_arg_type = vec![];
        for (name, exp) in func_call.dict_args.iter_mut() {
            dict_arg_type.push((name, self.parse_exp(exp)?));
        }
        let res_fun = fun_type.types.iter().find(|(args, _)| {
            if args.len() >= pos_arg_type.len() {
                let pos_match = pos_arg_type.iter().zip(args.iter()).all(|(x1, x2)| {
                    x1.syntax_type == x2.1 || implicity_convert(&x1.syntax_type, &x2.1)
                });
                let dict_match = dict_arg_type.iter().all(|(name, t)| {
                    match args.iter().find(|s| s.0 == name.value) {
                        None => false,
                        Some(val) => {
                            t.syntax_type == val.1 || implicity_convert(&t.syntax_type, &val.1)
                        }
                    }
                });
                pos_match && dict_match
            } else {
                false
            }
        });
        match res_fun {
            None => Err(PineInputError::new(
                PineErrorKind::FuncCallSignatureNotMatch,
                func_call.range,
            )),
            Some(d) => Ok(ParseValue::new_with_type(d.1.clone())),
        }
    }

    fn parse_user_func_call(
        &mut self,
        func_call: &mut FunctionCall<'a>,
        names: &Rc<Vec<&'a str>>,
        method_name: &'a str,
    ) -> ParseResult<'a> {
        if func_call.dict_args.len() > 0 {
            Err(PineInputError::new(
                PineErrorKind::ForbiddenDictArgsForUserFunc,
                func_call.range,
            ))
        } else if func_call.pos_args.len() != names.len() {
            Err(PineInputError::new(
                PineErrorKind::FuncCallSignatureNotMatch,
                func_call.range,
            ))
        } else {
            let mut pos_arg_type = vec![];
            for arg in func_call.pos_args.iter_mut() {
                pos_arg_type.push(self.parse_exp(arg)?);
            }
            let mut sub_ctx = Box::new(SyntaxContext::new(
                Some(self.context),
                ContextType::FuncDefBlock,
            ));
            names
                .iter()
                .zip(pos_arg_type.iter())
                .for_each(|(&n, t)| sub_ctx.declare_var(n, t.syntax_type.clone()));
            self.context = &mut *sub_ctx;

            let body;
            unsafe {
                body = &mut self.user_funcs[method_name].as_mut().unwrap().body;
            };
            let parse_res = self.parse_blk(body)?;

            self.context = sub_ctx.parent.unwrap();
            self.ctxs.insert(func_call.ctxid, sub_ctx);
            Ok(ParseValue::new_with_type(parse_res.syntax_type))
        }
    }

    fn parse_func_call(&mut self, func_call: &mut FunctionCall<'a>) -> ParseResult<'a> {
        let method_type = self.parse_exp(&mut func_call.method)?;
        if let SyntaxType::Function(fun_type) = method_type.syntax_type {
            self.parse_std_func_call(func_call, &fun_type)
        } else if let SyntaxType::UserFunction(names) = method_type.syntax_type {
            self.parse_user_func_call(func_call, &names, method_type.varname.unwrap())
        } else {
            Err(PineInputError::new(
                PineErrorKind::VarNotCallable,
                func_call.range,
            ))
        }
    }

    fn parse_tuple(&mut self, tuple: &mut TupleNode<'a>) -> ParseResult<'a> {
        let mut tuple_type: Vec<SyntaxType<'a>> = vec![];
        for arg in tuple.exps.iter_mut() {
            tuple_type.push(self.parse_exp(arg)?.syntax_type);
        }
        Ok(ParseValue::new_with_type(SyntaxType::Tuple(Rc::new(
            tuple_type,
        ))))
    }

    fn parse_ref_call_arg(
        &mut self,
        ref_call: &mut RefCall<'a>,
        name_type: SyntaxType<'a>,
    ) -> ParseResult<'a> {
        let arg_res = self.parse_exp(&mut ref_call.arg)?;
        match arg_res.syntax_type {
            SyntaxType::Simple(SimpleSyntaxType::Int)
            | SyntaxType::Series(SimpleSyntaxType::Int) => Ok(ParseValue::new_with_type(name_type)),
            _ => {
                self.catch(PineInputError::new(
                    PineErrorKind::RefIndexNotInt,
                    ref_call.range,
                ));
                Ok(ParseValue::new_with_type(name_type))
            }
        }
    }

    fn parse_ref_call(&mut self, ref_call: &mut RefCall<'a>) -> ParseResult<'a> {
        let name_res = self.parse_exp(&mut ref_call.name)?;

        match name_res.syntax_type {
            // If the type is simple, then we need to update the var to series.
            SyntaxType::Simple(t) => {
                if let Some(name) = name_res.varname {
                    let series_type = SyntaxType::Series(t);
                    downcast_ctx(self.context).update_var(name, series_type.clone());
                    self.parse_ref_call_arg(ref_call, series_type)
                } else {
                    Err(PineInputError::new(
                        PineErrorKind::VarNotSeriesInRef,
                        ref_call.range,
                    ))
                }
            }
            SyntaxType::Series(_) => self.parse_ref_call_arg(ref_call, name_res.syntax_type),
            _ => Err(PineInputError::new(
                PineErrorKind::VarNotSeriesInRef,
                ref_call.range,
            )),
        }
    }

    fn get_for_obj(
        obj: &SyntaxType<'a>,
        keys: &[&'a str],
    ) -> Result<SyntaxType<'a>, PineErrorKind> {
        if let SyntaxType::Object(map) = obj {
            match map.get(&keys[0]) {
                None => Err(PineErrorKind::RefKeyNotExist),
                Some(dest_type) => {
                    if keys.len() > 1 {
                        Self::get_for_obj(&dest_type, &keys[1..])
                    } else {
                        Ok(dest_type.clone())
                    }
                }
            }
        } else {
            Err(PineErrorKind::RefObjTypeNotObj)
        }
    }

    fn parse_prefix(&mut self, prefix: &mut PrefixExp<'a>) -> ParseResult<'a> {
        let subkeys: Vec<_> = prefix.var_chain.iter().map(|s| s.value).collect();
        match downcast_ctx(self.context).get_var(prefix.var_chain[0].value) {
            None => Err(PineInputError::new(
                PineErrorKind::VarNotDeclare,
                prefix.range,
            )),
            Some(obj) => match Self::get_for_obj(obj, &subkeys[1..]) {
                Ok(val_type) => Ok(ParseValue::new_with_type(val_type)),
                Err(code) => Err(PineInputError::new(code, prefix.range)),
            },
        }
    }

    fn parse_condition(&mut self, condition: &mut Condition<'a>) -> ParseResult<'a> {
        let cond_res = self.parse_exp(&mut condition.cond)?;
        if implicity_convert(
            &cond_res.syntax_type,
            &SyntaxType::Series(SimpleSyntaxType::Bool),
        ) {
            let exp1_res = self.parse_exp(&mut condition.exp1)?;
            let exp2_res = self.parse_exp(&mut condition.exp2)?;
            if implicity_convert(&exp1_res.syntax_type, &exp2_res.syntax_type) {
                Ok(ParseValue::new_with_type(simple_to_series(
                    exp2_res.syntax_type,
                )))
            } else if implicity_convert(&exp2_res.syntax_type, &exp1_res.syntax_type) {
                Ok(ParseValue::new_with_type(simple_to_series(
                    exp1_res.syntax_type,
                )))
            } else {
                Err(PineInputError::new(
                    PineErrorKind::CondExpTypesNotSame,
                    condition.range,
                ))
            }
        } else {
            Err(PineInputError::new(
                PineErrorKind::CondNotBool,
                condition.range,
            ))
        }
    }

    fn parse_ifthenelse_cond(&mut self, ite: &mut IfThenElse<'a>) -> Result<(), PineInputError> {
        let cond_res = self.parse_exp(&mut ite.cond)?;
        if !implicity_convert(
            &cond_res.syntax_type,
            &SyntaxType::Series(SimpleSyntaxType::Bool),
        ) {
            return Err(PineInputError::new(PineErrorKind::CondNotBool, ite.range));
        }
        Ok(())
    }

    fn parse_ifthenelse_then(&mut self, ite: &mut IfThenElse<'a>) -> ParseResult<'a> {
        // Create new context for if block
        let mut if_ctx = Box::new(SyntaxContext::new(
            Some(self.context),
            ContextType::IfElseBlock,
        ));
        self.context = &mut *if_ctx;

        let then_res = self.parse_blk(&mut ite.then_blk)?;
        self.context = if_ctx.parent.unwrap();
        self.ctxs.insert(ite.then_ctxid, if_ctx);
        Ok(then_res)
    }

    fn parse_ifthenelse_else(&mut self, else_blk: &mut Block<'a>, ctxid: i32) -> ParseResult<'a> {
        // Create new context for if block
        let mut else_ctx = Box::new(SyntaxContext::new(
            Some(self.context),
            ContextType::IfElseBlock,
        ));
        self.context = &mut *else_ctx;

        let else_res = self.parse_blk(else_blk)?;
        self.context = else_ctx.parent.unwrap();
        self.ctxs.insert(ctxid, else_ctx);
        Ok(else_res)
    }

    fn parse_ifthenelse_exp(&mut self, ite: &mut IfThenElse<'a>) -> ParseResult<'a> {
        self.parse_ifthenelse_cond(ite)?;
        let then_res = self.parse_ifthenelse_then(ite)?;

        if then_res.syntax_type.is_void() {
            return Err(PineInputError::new(
                PineErrorKind::ExpNoReturn,
                ite.then_blk.range,
            ));
        }
        if then_res.syntax_type.is_na() {
            return Err(PineInputError::new(
                PineErrorKind::ExpReturnNa,
                ite.then_blk.range,
            ));
        }
        if let Some(else_blk) = &mut ite.else_blk {
            // Create new context for if block
            let else_res = self.parse_ifthenelse_else(else_blk, ite.else_ctxid)?;

            if else_res.syntax_type.is_void() {
                return Err(PineInputError::new(
                    PineErrorKind::ExpNoReturn,
                    else_blk.range,
                ));
            }
            if else_res.syntax_type.is_na() {
                return Err(PineInputError::new(
                    PineErrorKind::ExpReturnNa,
                    else_blk.range,
                ));
            }
            // Find the common type that can satisfy the then type and else type
            if implicity_convert(&then_res.syntax_type, &else_res.syntax_type) {
                // The return type of if-then-else block must be series.
                return Ok(ParseValue::new_with_type(simple_to_series(
                    else_res.syntax_type,
                )));
            } else if implicity_convert(&else_res.syntax_type, &then_res.syntax_type) {
                return Ok(ParseValue::new_with_type(simple_to_series(
                    then_res.syntax_type,
                )));
            } else {
                return Err(PineInputError::new(PineErrorKind::TypeMismatch, ite.range));
            }
        } else {
            Ok(ParseValue::new_with_type(then_res.syntax_type))
        }
    }

    fn parse_ifthenelse_stmt(&mut self, ite: &mut IfThenElse<'a>) -> ParseResult<'a> {
        self.parse_ifthenelse_cond(ite)?;
        self.parse_ifthenelse_then(ite)?;
        if let Some(else_blk) = &mut ite.else_blk {
            self.parse_ifthenelse_else(else_blk, ite.else_ctxid)?;
        }
        Ok(ParseValue::new_with_type(SyntaxType::Void))
    }

    fn parse_forrange(&mut self, for_range: &mut ForRange<'a>) -> ParseResult<'a> {
        let start_res = self.parse_exp(&mut for_range.start)?;
        if !start_res.syntax_type.is_int() {
            self.catch(PineInputError::new(
                PineErrorKind::ForRangeIndexNotInt,
                for_range.start.range(),
            ));
        }
        let end_res = self.parse_exp(&mut for_range.end)?;
        if !end_res.syntax_type.is_int() {
            self.catch(PineInputError::new(
                PineErrorKind::ForRangeIndexNotInt,
                for_range.end.range(),
            ));
        }
        if let Some(step) = &mut for_range.step {
            let step_res = self.parse_exp(step)?;
            if !step_res.syntax_type.is_int() {
                self.catch(PineInputError::new(
                    PineErrorKind::ForRangeIndexNotInt,
                    step.range(),
                ));
            }
        }
        let mut for_ctx = Box::new(SyntaxContext::new(
            Some(self.context),
            ContextType::ForRangeBlock,
        ));
        for_ctx.declare_var(
            for_range.var.value,
            SyntaxType::Simple(SimpleSyntaxType::Int),
        );
        self.context = &mut *for_ctx;

        let blk_res = self.parse_blk(&mut for_range.do_blk)?;
        self.context = for_ctx.parent.unwrap();
        self.ctxs.insert(for_range.ctxid, for_ctx);
        Ok(blk_res)
    }

    fn parse_forrange_stmt(&mut self, for_range: &mut ForRange<'a>) -> ParseResult<'a> {
        self.parse_forrange(for_range)?;
        Ok(ParseValue::new_with_type(SyntaxType::Void))
    }

    fn parse_forrange_exp(&mut self, for_range: &mut ForRange<'a>) -> ParseResult<'a> {
        let blk_res = self.parse_forrange(for_range)?;
        if blk_res.syntax_type.is_void() {
            return Err(PineInputError::new(
                PineErrorKind::ExpNoReturn,
                for_range.do_blk.range,
            ));
        }
        if blk_res.syntax_type.is_na() {
            return Err(PineInputError::new(
                PineErrorKind::ExpReturnNa,
                for_range.do_blk.range,
            ));
        }
        Ok(ParseValue::new_with_type(simple_to_series(
            blk_res.syntax_type,
        )))
    }

    fn parse_varname(&mut self, varname: &mut VarName<'a>) -> ParseResult<'a> {
        match downcast_ctx(self.context).get_var(varname.value) {
            None => Err(PineInputError::new(
                PineErrorKind::VarNotDeclare,
                varname.range,
            )),
            Some(val) => Ok(ParseValue::new(val.clone(), varname.value)),
        }
    }

    fn parse_type_cast(&mut self, type_cast: &mut TypeCast<'a>) -> ParseResult<'a> {
        let origin_type = self.parse_exp(&mut type_cast.exp)?.syntax_type;
        let (is_cast_err, result) = explicity_type_cast(&origin_type, &type_cast.data_type);
        if is_cast_err {
            self.catch(PineInputError::new(
                PineErrorKind::InvalidTypeCast {
                    origin: format!("{:?}", origin_type),
                    cast: format!("{:?}", type_cast.data_type),
                },
                type_cast.range,
            ));
        }
        Ok(ParseValue::new_with_type(result))
    }

    fn parse_unary(&mut self, unary: &mut UnaryExp<'a>) -> ParseResult<'a> {
        let exp_type = self.parse_exp(&mut unary.exp)?.syntax_type;
        match unary.op {
            UnaryOp::Plus | UnaryOp::Minus => {
                if !exp_type.is_num() {
                    Err(PineInputError::new(
                        PineErrorKind::UnaryTypeNotNum,
                        unary.range,
                    ))
                } else {
                    Ok(ParseValue::new_with_type(exp_type))
                }
            }
            UnaryOp::BoolNot => {
                if implicity_convert(&exp_type, &SyntaxType::Simple(SimpleSyntaxType::Bool)) {
                    Ok(ParseValue::new_with_type(SyntaxType::Simple(
                        SimpleSyntaxType::Bool,
                    )))
                } else if implicity_convert(&exp_type, &SyntaxType::Series(SimpleSyntaxType::Bool))
                {
                    Ok(ParseValue::new_with_type(SyntaxType::Series(
                        SimpleSyntaxType::Bool,
                    )))
                } else {
                    Err(PineInputError::new(
                        PineErrorKind::UnaryTypeNotNum,
                        unary.range,
                    ))
                }
            }
        }
    }

    pub fn parse_binary(&mut self, binary: &mut BinaryExp<'a>) -> ParseResult<'a> {
        let exp1_type = self.parse_exp(&mut binary.exp1)?.syntax_type;
        let exp2_type = self.parse_exp(&mut binary.exp2)?.syntax_type;
        let gen_bool = |binary: &mut BinaryExp<'a>, exp1_type, exp2_type| {
            let result = match (exp1_type, exp2_type) {
                (SyntaxType::Series(_), _) | (_, SyntaxType::Series(_)) => {
                    SyntaxType::Series(SimpleSyntaxType::Bool)
                }
                _ => SyntaxType::Simple(SimpleSyntaxType::Bool),
            };
            binary.result_type = result.clone();
            Ok(ParseValue::new_with_type(result))
        };
        match binary.op {
            BinaryOp::Plus | BinaryOp::Minus | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                // plus for string concatenate
                if binary.op == BinaryOp::Plus && exp1_type.is_string() && exp2_type.is_string() {
                    let result_type = match (exp1_type, exp2_type) {
                        (SyntaxType::Series(_), _) | (_, SyntaxType::Series(_)) => {
                            SyntaxType::Series(SimpleSyntaxType::String)
                        }
                        _ => SyntaxType::Simple(SimpleSyntaxType::String),
                    };
                    binary.result_type = result_type.clone();
                    return Ok(ParseValue::new_with_type(result_type));
                }
                // The operator must be num type int, float, series(int), series(float)
                if !(exp1_type.is_num() && exp2_type.is_num()) {
                    return Err(PineInputError::new(
                        PineErrorKind::BinaryTypeNotNum,
                        binary.range,
                    ));
                }
                if let Some(result_type) = similar_type(&exp1_type, &exp2_type) {
                    binary.result_type = result_type.clone();
                    Ok(ParseValue::new_with_type(exp2_type))
                } else {
                    Err(PineInputError::new(
                        PineErrorKind::TypeMismatch,
                        binary.range,
                    ))
                }
            }
            BinaryOp::Gt | BinaryOp::Geq | BinaryOp::Lt | BinaryOp::Leq => {
                if !(exp1_type.is_num() && exp2_type.is_num()) {
                    self.catch(PineInputError::new(
                        PineErrorKind::BinaryTypeNotNum,
                        binary.range,
                    ));
                }
                gen_bool(binary, exp1_type, exp2_type)
            }
            BinaryOp::Eq | BinaryOp::Neq => {
                if let Some(_type) = similar_type(&exp2_type, &exp1_type) {
                    binary.ref_type = _type;
                    gen_bool(binary, exp1_type, exp2_type)
                } else {
                    self.catch(PineInputError::new(
                        PineErrorKind::TypeMismatch,
                        binary.range,
                    ));
                    gen_bool(binary, exp1_type, exp2_type)
                }
            }
            BinaryOp::BoolAnd | BinaryOp::BoolOr => {
                let bool_type = SyntaxType::Series(SimpleSyntaxType::Bool);
                if implicity_convert(&exp1_type, &bool_type)
                    && implicity_convert(&exp2_type, &bool_type)
                {
                    gen_bool(binary, exp1_type, exp2_type)
                } else {
                    self.catch(PineInputError::new(
                        PineErrorKind::BoolExpTypeNotBool,
                        binary.range,
                    ));
                    gen_bool(binary, exp1_type, exp2_type)
                }
            }
        }
    }

    fn parse_one_assign(
        &mut self,
        var_type: &Option<DataType>,
        name: &VarName<'a>,
        val: SyntaxType<'a>,
        range: StrRange,
    ) -> Result<SyntaxType<'a>, PineInputError> {
        let context = downcast_ctx(self.context);
        if context.get_var_scope(name.value).is_some() {
            self.catch(PineInputError::new(
                PineErrorKind::VarHasDeclare,
                name.range,
            ));
        }
        if let Some(data_type) = var_type {
            let (is_cast_err, result) = implicity_type_cast(&val, &data_type);
            if is_cast_err {
                self.catch(PineInputError::new(
                    PineErrorKind::InvalidTypeCast {
                        origin: format!("{:?}", val),
                        cast: format!("{:?}", data_type),
                    },
                    range,
                ));
            }
            context.declare_var(name.value, result.clone());
            Ok(result)
        } else {
            context.declare_var(name.value, val.clone());
            Ok(val)
        }
    }

    fn parse_assign(&mut self, assign: &mut Assignment<'a>) -> ParseResult<'a> {
        let val_res = self.parse_exp(&mut assign.val)?;
        if assign.names.len() > 1 {
            if let SyntaxType::Tuple(tuple) = val_res.syntax_type {
                if tuple.len() != assign.names.len() {
                    Err(PineInputError::new(
                        PineErrorKind::BinaryTypeNotNum,
                        assign.range,
                    ))
                } else {
                    let mut ret_tuple = vec![];
                    for (name, val_type) in assign.names.iter().zip(tuple.iter()) {
                        ret_tuple.push(self.parse_one_assign(
                            &assign.var_type,
                            name,
                            val_type.clone(),
                            assign.range,
                        )?);
                    }
                    Ok(ParseValue::new_with_type(SyntaxType::Tuple(Rc::new(
                        ret_tuple,
                    ))))
                }
            } else {
                Err(PineInputError::new(
                    PineErrorKind::BinaryTypeNotNum,
                    assign.range,
                ))
            }
        } else {
            let rtype = self.parse_one_assign(
                &assign.var_type,
                &assign.names[0],
                val_res.syntax_type,
                assign.range,
            )?;
            Ok(ParseValue::new_with_type(rtype))
        }
    }

    fn parse_var_assign(&mut self, assign: &mut VarAssignment<'a>) -> ParseResult<'a> {
        let context = downcast_ctx(self.context);
        match context.get_var(assign.name.value) {
            None => Err(PineInputError::new(
                PineErrorKind::VarNotDeclare,
                assign.range,
            )),
            Some(cur_type) => {
                let last_type = simple_to_series(cur_type.clone());
                let val_res = self.parse_exp(&mut assign.val)?;
                if implicity_convert(&val_res.syntax_type, &last_type) {
                    Ok(ParseValue::new_with_type(last_type))
                } else {
                    self.catch(PineInputError::new(
                        PineErrorKind::InvalidTypeCast {
                            origin: format!("{:?}", val_res.syntax_type),
                            cast: format!("{:?}", last_type),
                        },
                        assign.range,
                    ));
                    Ok(ParseValue::new_with_type(last_type))
                }
            }
        }
    }

    fn parse_func_def(&mut self, func_def: &mut FunctionDef<'a>) -> ParseResult<'a> {
        let context = downcast_ctx(self.context);
        let name = func_def.name.value;
        if context.get_var_scope(name).is_some() {
            self.catch(PineInputError::new(
                PineErrorKind::VarHasDeclare,
                func_def.name.range,
            ));
        }

        self.user_funcs.insert(name, func_def);
        let param_names: Vec<_> = func_def.params.iter().map(|v| v.value).collect();
        let name_type = SyntaxType::UserFunction(Rc::new(param_names));
        context.declare_var(name, name_type.clone());
        Ok(ParseValue::new_with_type(name_type))
    }

    fn parse_exp(&mut self, exp: &mut Exp<'a>) -> ParseResult<'a> {
        match exp {
            Exp::Na(_) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Na,
            ))),
            Exp::Bool(_) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool,
            ))),
            Exp::Num(Numeral::Int(_)) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Int,
            ))),
            Exp::Num(Numeral::Float(_)) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Float,
            ))),
            Exp::Str(_) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::String,
            ))),
            Exp::Color(_) => Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Color,
            ))),
            Exp::VarName(name) => self.parse_varname(name),
            Exp::Tuple(tuple) => self.parse_tuple(tuple),
            Exp::TypeCast(type_cast) => self.parse_type_cast(type_cast),
            Exp::FuncCall(func_call) => self.parse_func_call(func_call),
            Exp::RefCall(ref_call) => self.parse_ref_call(ref_call),
            Exp::PrefixExp(prefix) => self.parse_prefix(prefix),
            Exp::Condition(condition) => self.parse_condition(condition),
            Exp::Ite(ite) => self.parse_ifthenelse_exp(ite),
            Exp::ForRange(fr) => self.parse_forrange_exp(fr),
            Exp::UnaryExp(node) => self.parse_unary(node),
            Exp::BinaryExp(node) => self.parse_binary(node),
        }
    }

    fn parse_interrupt(&mut self, range: &StrRange, code: PineErrorKind) -> ParseResult<'a> {
        let context = downcast_ctx(self.context);
        if context.get_type() != ContextType::ForRangeBlock {
            self.catch(PineInputError::new(code, *range));
        }
        Ok(ParseValue::new_with_type(SyntaxType::Void))
    }

    fn parse_break(&mut self, range: &StrRange) -> ParseResult<'a> {
        self.parse_interrupt(range, PineErrorKind::BreakNotInForStmt)
    }

    fn parse_continue(&mut self, range: &StrRange) -> ParseResult<'a> {
        self.parse_interrupt(range, PineErrorKind::ContinueNotInForStmt)
    }

    fn parse_stmt(&mut self, stmt: &mut Statement<'a>) -> ParseResult<'a> {
        match stmt {
            Statement::Break(node) => self.parse_break(node),
            Statement::Continue(node) => self.parse_continue(node),
            Statement::FuncCall(func_call) => self.parse_func_call(func_call),
            Statement::Ite(ite) => self.parse_ifthenelse_stmt(ite),
            Statement::ForRange(fr) => self.parse_forrange_stmt(fr),
            Statement::Assignment(assign) => self.parse_assign(assign),
            Statement::VarAssignment(assign) => self.parse_var_assign(assign),
            Statement::FuncDef(func_def) => self.parse_func_def(func_def),
            Statement::None(_) => Ok(ParseValue::new_with_type(SyntaxType::Void)),
        }
    }

    pub fn parse_blk(&mut self, blk: &mut Block<'a>) -> ParseResult<'a> {
        for stmt in blk.stmts.iter_mut() {
            self.parse_stmt(stmt)?;
        }
        if let Some(ref mut exp) = blk.ret_stmt {
            self.parse_exp(exp)
        } else {
            Ok(ParseValue::new_with_type(SyntaxType::Void))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::color::ColorNode;
    use crate::ast::input::{Input, Position, StrRange};
    use crate::ast::name::VarName;
    use crate::ast::stat_expr_types::{BoolNode, DataType, NaNode, RefCall, TypeCast};
    use crate::ast::string::StringNode;

    fn varname(n: &str) -> VarName {
        VarName::new(n, StrRange::new_empty())
    }

    #[test]
    fn func_def_test() {
        let mut parser = SyntaxParser::new();
        let mut func_def = FunctionDef {
            name: varname("funa"),
            params: vec![varname("hello"), varname("hello2")],
            body: Block::new(vec![], None, StrRange::new_empty()),
            range: StrRange::new_empty(),
        };
        assert_eq!(
            parser.parse_func_def(&mut func_def),
            Ok(ParseValue::new_with_type(SyntaxType::UserFunction(
                Rc::new(vec!["hello", "hello2"])
            )))
        );
        assert_eq!(parser.user_funcs["funa"], &mut func_def as *mut FunctionDef);
        assert_eq!(downcast_ctx(parser.context).vars.contains_key("funa"), true);
    }

    const INT_TYPE: SyntaxType = SyntaxType::Simple(SimpleSyntaxType::Int);
    const FLOAT_TYPE: SyntaxType = SyntaxType::Simple(SimpleSyntaxType::Float);

    fn int_exp<'a>(i: i32) -> Exp<'a> {
        Exp::Num(Numeral::from_i32(i))
    }

    fn float_exp<'a>(i: f64) -> Exp<'a> {
        Exp::Num(Numeral::from_f64(i))
    }

    fn fun_nm<'a>() -> Exp<'a> {
        Exp::VarName(varname("func"))
    }

    #[test]
    fn func_call_test() {
        let mut parser = SyntaxParser::new();
        downcast_ctx(parser.context).declare_var(
            "func",
            SyntaxType::Function(Rc::new(FunctionType {
                types: vec![
                    (vec![("arg1", INT_TYPE), ("arg2", INT_TYPE)], INT_TYPE),
                    (vec![("arg1", FLOAT_TYPE), ("arg2", FLOAT_TYPE)], FLOAT_TYPE),
                ],
            })),
        );

        let mut func_call = FunctionCall::new(
            fun_nm(),
            vec![int_exp(1), int_exp(2)],
            vec![],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(INT_TYPE))
        );

        let mut func_call =
            FunctionCall::new(fun_nm(), vec![int_exp(1)], vec![], 1, StrRange::new_empty());
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(INT_TYPE))
        );

        let mut func_call = FunctionCall::new(
            fun_nm(),
            vec![int_exp(1)],
            vec![(varname("arg2"), int_exp(1))],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(INT_TYPE))
        );

        let mut func_call = FunctionCall::new(
            fun_nm(),
            vec![int_exp(1)],
            vec![(varname("arg2"), float_exp(1f64))],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(FLOAT_TYPE))
        );

        let mut func_call = FunctionCall::new(
            fun_nm(),
            vec![float_exp(1f64)],
            vec![(varname("arg2"), int_exp(1))],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(FLOAT_TYPE))
        );

        let mut func_call = FunctionCall::new(
            fun_nm(),
            vec![float_exp(1f64)],
            vec![(varname("arg3"), int_exp(1))],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Err(PineInputError::new(
                PineErrorKind::FuncCallSignatureNotMatch,
                StrRange::new_empty()
            ))
        );
    }

    #[test]
    fn user_func_call_test() {
        let mut parser = SyntaxParser::new();
        let mut func_def = FunctionDef {
            name: varname("fun"),
            params: vec![varname("a1"), varname("a2")],
            body: Block::new(
                vec![],
                Some(Exp::VarName(varname("a2"))),
                StrRange::new_empty(),
            ),
            range: StrRange::new_empty(),
        };
        assert!(parser.parse_func_def(&mut func_def).is_ok());

        let mut func_call = FunctionCall::new(
            Exp::VarName(varname("fun")),
            vec![int_exp(1), float_exp(2f64)],
            vec![],
            1,
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_func_call(&mut func_call),
            Ok(ParseValue::new_with_type(FLOAT_TYPE))
        );
        assert_eq!(downcast_ctx(parser.context).parent, None);
        assert_eq!(
            downcast_ctx(&mut **parser.ctxs.get_mut(&1).unwrap())
                .vars
                .contains_key("a1"),
            true
        );
    }

    #[test]
    fn simple_test() {
        let mut parser = SyntaxParser::new();
        assert_eq!(
            parser.parse_exp(&mut Exp::Na(NaNode::new(StrRange::new_empty()))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Na
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Bool(BoolNode::new_no_range(true))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Num(Numeral::from_f64(1f64))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Float
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Num(Numeral::from_i32(1))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Int
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Color(ColorNode::from_str("#AAAAAA"))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Color
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Str(StringNode::new(
                String::from("hello"),
                StrRange::new_empty()
            ))),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::String
            )))
        );
    }

    #[test]
    fn varname_test() {
        let mut parser = SyntaxParser::new();
        downcast_ctx(parser.context).declare_var("hello", INT_TYPE);
        assert_eq!(
            parser.parse_exp(&mut Exp::VarName(varname("hello"))),
            Ok(ParseValue::new(INT_TYPE, "hello"))
        );
    }

    #[test]
    fn tuple_test() {
        let mut parser = SyntaxParser::new();
        downcast_ctx(parser.context).declare_var("arg1", INT_TYPE);
        downcast_ctx(parser.context).declare_var("arg2", FLOAT_TYPE);
        assert_eq!(
            parser.parse_exp(&mut Exp::Tuple(Box::new(TupleNode::new(
                vec![Exp::VarName(varname("arg1")), Exp::VarName(varname("arg2"))],
                StrRange::new_empty()
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Tuple(Rc::new(vec![
                INT_TYPE, FLOAT_TYPE
            ]))))
        );
    }

    #[test]
    fn tuple_type_cast() {
        let mut parser = SyntaxParser::new();
        downcast_ctx(parser.context)
            .declare_var("arg1", SyntaxType::Series(SimpleSyntaxType::Float));

        assert_eq!(
            parser.parse_exp(&mut Exp::TypeCast(Box::new(TypeCast::new_no_input(
                DataType::Int,
                Exp::VarName(varname("arg1"))
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Int
            )))
        );
    }

    #[test]
    fn ref_call_cast() {
        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("series", SyntaxType::Series(SimpleSyntaxType::Float));
        context.declare_var("simple", SyntaxType::Simple(SimpleSyntaxType::Float));
        context.declare_var("arg", SyntaxType::Series(SimpleSyntaxType::Float));

        assert_eq!(
            parser.parse_exp(&mut Exp::RefCall(Box::new(RefCall::new_no_input(
                Exp::VarName(varname("series")),
                Exp::VarName(varname("arg"))
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Float
            )))
        );
        assert_eq!(parser.errors.len(), 1);

        assert_eq!(
            parser.parse_exp(&mut Exp::RefCall(Box::new(RefCall::new_no_input(
                Exp::VarName(varname("simple")),
                Exp::VarName(varname("arg"))
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Float
            )))
        );
        assert_eq!(
            context.get_var_scope("simple"),
            Some(&SyntaxType::Series(SimpleSyntaxType::Float))
        );
    }

    #[test]
    fn condition_test() {
        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("cond", SyntaxType::Series(SimpleSyntaxType::Float));
        context.declare_var("arg1", SyntaxType::Series(SimpleSyntaxType::Float));
        context.declare_var("arg2", SyntaxType::Series(SimpleSyntaxType::Int));

        assert_eq!(
            parser.parse_exp(&mut Exp::Condition(Box::new(Condition::new_no_input(
                Exp::VarName(varname("cond")),
                Exp::VarName(varname("arg1")),
                Exp::VarName(varname("arg2"))
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Float
            )))
        );

        assert_eq!(
            parser.parse_exp(&mut Exp::Condition(Box::new(Condition::new_no_input(
                Exp::Num(Numeral::from_i32(1)),
                Exp::Num(Numeral::from_i32(2)),
                Exp::Num(Numeral::from_i32(3))
            )))),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Int
            )))
        );
    }

    #[test]
    fn if_then_else_exp_test() {
        use crate::ast::stat_expr::if_then_else_exp;
        use crate::ast::state::AstState;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("var", SyntaxType::Simple(SimpleSyntaxType::Int));

        let input = Input::new_with_str("if var\n    1\nelse\n    1.0");
        assert_eq!(
            parser
                .parse_ifthenelse_exp(&mut if_then_else_exp(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Float
            )))
        );

        // let input = Input::new_with_str("if var\n    var = 1\n    var\nelse\n    var=1.0\n    var");
        // assert_eq!(
        //     parser
        //         .parse_ifthenelse_exp(&mut if_then_else_exp(0)(input, &AstState::new()).unwrap().1),
        //     Ok(ParseValue::new_with_type(SyntaxType::Simple(
        //         SimpleSyntaxType::Float
        //     )))
        // )
    }

    #[test]
    fn for_range_exp_test() {
        use crate::ast::stat_expr::for_range_exp;
        use crate::ast::state::AstState;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("var", SyntaxType::Simple(SimpleSyntaxType::Int));

        let input = Input::new_with_str("for i = 1 to 2\n    i");
        assert_eq!(
            parser.parse_forrange_exp(&mut for_range_exp(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Int
            )))
        );
    }

    #[test]
    fn unary_exp_test() {
        use crate::ast::stat_expr_types::UnaryExp;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);

        let mut plus_exp = UnaryExp::new(UnaryOp::Plus, int_exp(1), StrRange::new_empty());
        assert_eq!(
            parser.parse_unary(&mut plus_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Int
            )))
        );

        context.declare_var("var", SyntaxType::Series(SimpleSyntaxType::Int));
        let mut series_plus_exp = UnaryExp::new(
            UnaryOp::Plus,
            Exp::VarName(varname("var")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_unary(&mut series_plus_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Int
            )))
        );

        let mut na_plus_exp = UnaryExp::new(
            UnaryOp::Plus,
            Exp::Na(NaNode::new(StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_unary(&mut na_plus_exp),
            Err(PineInputError::new(
                PineErrorKind::UnaryTypeNotNum,
                StrRange::new_empty()
            ))
        );

        let mut bool_exp = UnaryExp::new(UnaryOp::BoolNot, int_exp(1), StrRange::new_empty());
        assert_eq!(
            parser.parse_unary(&mut bool_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );

        context.declare_var("var", SyntaxType::Series(SimpleSyntaxType::Int));
        let mut series_plus_exp = UnaryExp::new(
            UnaryOp::BoolNot,
            Exp::VarName(varname("var")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_unary(&mut series_plus_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Bool
            )))
        );
    }

    #[test]
    fn add_binary_exp_test() {
        use crate::ast::string::StringNode;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);

        let mut str_add_exp = BinaryExp::new(
            BinaryOp::Plus,
            Exp::Str(StringNode::new(String::from("he"), StrRange::new_empty())),
            Exp::Str(StringNode::new(String::from("llo"), StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut str_add_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::String
            )))
        );
        assert_eq!(
            str_add_exp.result_type,
            SyntaxType::Simple(SimpleSyntaxType::String)
        );

        context.declare_var("str1", SyntaxType::Series(SimpleSyntaxType::String));
        context.declare_var("str2", SyntaxType::Series(SimpleSyntaxType::String));
        let mut sstr_add_exp = BinaryExp::new(
            BinaryOp::Plus,
            Exp::VarName(varname("str1")),
            Exp::VarName(varname("str2")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut sstr_add_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::String
            )))
        );
        assert_eq!(
            sstr_add_exp.result_type,
            SyntaxType::Series(SimpleSyntaxType::String)
        );

        let mut int_add_exp = BinaryExp::new(
            BinaryOp::Plus,
            int_exp(1),
            int_exp(2),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut int_add_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Int
            )))
        );
        assert_eq!(
            int_add_exp.result_type,
            SyntaxType::Simple(SimpleSyntaxType::Int)
        );

        context.declare_var("sint", SyntaxType::Series(SimpleSyntaxType::Int));
        let mut int_add_exp = BinaryExp::new(
            BinaryOp::Plus,
            int_exp(1),
            Exp::VarName(varname("sint")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut int_add_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Int
            )))
        );
        assert_eq!(
            int_add_exp.result_type,
            SyntaxType::Series(SimpleSyntaxType::Int)
        );

        let mut na_add_exp = BinaryExp::new(
            BinaryOp::Plus,
            int_exp(1),
            Exp::Na(NaNode::new(StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut na_add_exp),
            Err(PineInputError::new(
                PineErrorKind::BinaryTypeNotNum,
                StrRange::new_empty()
            ))
        );
        assert_eq!(na_add_exp.result_type, SyntaxType::Any);
    }

    #[test]
    fn eq_exp_test() {
        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("sint", SyntaxType::Series(SimpleSyntaxType::Int));

        let mut eq_exp = BinaryExp::new(
            BinaryOp::Eq,
            int_exp(1),
            Exp::VarName(varname("sint")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut eq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(eq_exp.ref_type, SyntaxType::Series(SimpleSyntaxType::Int));

        let mut eq_exp = BinaryExp::new(
            BinaryOp::Eq,
            float_exp(1f64),
            Exp::VarName(varname("sint")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut eq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(eq_exp.ref_type, SyntaxType::Series(SimpleSyntaxType::Float));

        let mut eq_exp = BinaryExp::new(
            BinaryOp::Eq,
            int_exp(1),
            Exp::Na(NaNode::new(StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut eq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );
        assert!(!parser.errors.is_empty());
        assert_eq!(
            eq_exp.result_type,
            SyntaxType::Simple(SimpleSyntaxType::Bool)
        );

        let mut eq_dif_type_exp = BinaryExp::new(
            BinaryOp::Eq,
            int_exp(1),
            Exp::Str(StringNode::new(String::from("h"), StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut eq_dif_type_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(
            *parser.errors.last().unwrap(),
            PineInputError::new(PineErrorKind::TypeMismatch, StrRange::new_empty())
        );
    }

    #[test]
    fn binary_exp_test() {
        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);

        // Geq
        context.declare_var("sint", SyntaxType::Series(SimpleSyntaxType::Int));
        let mut geq_exp =
            BinaryExp::new(BinaryOp::Geq, int_exp(1), int_exp(2), StrRange::new_empty());
        assert_eq!(
            parser.parse_binary(&mut geq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(
            geq_exp.result_type,
            SyntaxType::Simple(SimpleSyntaxType::Bool)
        );

        let mut series_geq_exp = BinaryExp::new(
            BinaryOp::Geq,
            int_exp(1),
            Exp::VarName(varname("sint")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut series_geq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(
            series_geq_exp.result_type,
            SyntaxType::Series(SimpleSyntaxType::Bool)
        );

        let mut na_geq_exp = BinaryExp::new(
            BinaryOp::Geq,
            int_exp(1),
            Exp::Na(NaNode::new(StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut na_geq_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(
            *parser.errors.last().unwrap(),
            PineInputError::new(PineErrorKind::BinaryTypeNotNum, StrRange::new_empty())
        );

        // BoolAnd
        context.declare_var("var", SyntaxType::Series(SimpleSyntaxType::Int));
        let mut bool_and_exp = BinaryExp::new(
            BinaryOp::BoolAnd,
            int_exp(1),
            Exp::VarName(varname("var")),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut bool_and_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Series(
                SimpleSyntaxType::Bool
            )))
        );
        let mut bool_and_exp = BinaryExp::new(
            BinaryOp::BoolAnd,
            int_exp(1),
            Exp::Str(StringNode::new(String::from("h"), StrRange::new_empty())),
            StrRange::new_empty(),
        );
        assert_eq!(
            parser.parse_binary(&mut bool_and_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Bool
            )))
        );
        assert_eq!(
            *parser.errors.last().unwrap(),
            PineInputError::new(PineErrorKind::BoolExpTypeNotBool, StrRange::new_empty())
        );
    }

    #[test]
    fn prefix_exp_test() {
        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);

        let map2: HashMap<_, _> = [("key2", SyntaxType::Simple(SimpleSyntaxType::Int))]
            .iter()
            .cloned()
            .collect();
        let map1: HashMap<_, _> = [("key1", SyntaxType::Object(Rc::new(map2)))]
            .iter()
            .cloned()
            .collect();
        context.declare_var("var", SyntaxType::Object(Rc::new(map1)));

        let mut prefix_exp =
            PrefixExp::new_no_input(vec![varname("var"), varname("key1"), varname("key2")]);
        assert_eq!(
            parser.parse_prefix(&mut prefix_exp),
            Ok(ParseValue::new_with_type(SyntaxType::Simple(
                SimpleSyntaxType::Int
            )))
        );

        let mut prefix_exp = PrefixExp::new_no_input(vec![varname("var"), varname("nokey")]);
        assert_eq!(
            parser.parse_prefix(&mut prefix_exp),
            Err(PineInputError::new(
                PineErrorKind::RefKeyNotExist,
                StrRange::new_empty()
            ))
        );

        let mut prefix_exp = PrefixExp::new_no_input(vec![varname("var2"), varname("nokey")]);
        assert_eq!(
            parser.parse_prefix(&mut prefix_exp),
            Err(PineInputError::new(
                PineErrorKind::VarNotDeclare,
                StrRange::new_empty()
            ))
        );
    }

    #[test]
    fn if_then_else_stmt_test() {
        use crate::ast::stat_expr::if_then_else_with_indent;
        use crate::ast::state::AstState;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("var", SyntaxType::Simple(SimpleSyntaxType::Int));

        let input = Input::new_with_str("if var\n    1\nelse\n    na");
        assert_eq!(
            parser.parse_ifthenelse_stmt(
                &mut if_then_else_with_indent(0)(input, &AstState::new())
                    .unwrap()
                    .1
            ),
            Ok(ParseValue::new_with_type(SyntaxType::Void))
        );
    }

    #[test]
    fn assign_test() {
        use crate::ast::stat_expr::assign_with_indent;
        use crate::ast::state::AstState;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("var", SyntaxType::Simple(SimpleSyntaxType::Int));

        let input = Input::new_with_str("a = 1");
        let val_type = SyntaxType::Simple(SimpleSyntaxType::Int);
        assert_eq!(
            parser.parse_assign(&mut assign_with_indent(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(val_type.clone()))
        );
        assert_eq!(context.get_var_scope("a"), Some(&val_type));

        assert_eq!(
            parser.parse_assign(&mut assign_with_indent(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(val_type.clone()))
        );
        assert_eq!(
            parser.errors.last(),
            Some(&PineInputError::new(
                PineErrorKind::VarHasDeclare,
                StrRange::from_start("a", Position::new(0, 0))
            ))
        );

        let input = Input::new_with_str("[a1, a2] = [1, 2]");
        assert_eq!(
            parser.parse_assign(&mut assign_with_indent(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(SyntaxType::Tuple(Rc::new(vec![
                val_type.clone(),
                val_type.clone()
            ]))))
        );

        let input = Input::new_with_str("int [a1, a2] = [1.0, 2.0]");
        assert_eq!(
            parser.parse_assign(&mut assign_with_indent(0)(input, &AstState::new()).unwrap().1),
            Ok(ParseValue::new_with_type(SyntaxType::Tuple(Rc::new(vec![
                val_type.clone(),
                val_type.clone()
            ]))))
        );
        assert_matches!(
            parser.errors.last().unwrap(),
            &PineInputError {
                code: PineErrorKind::InvalidTypeCast { origin: _, cast: _ },
                range: _,
            }
        );
    }

    #[test]
    fn var_assign_test() {
        use crate::ast::stat_expr::var_assign_with_indent;
        use crate::ast::state::AstState;

        let mut parser = SyntaxParser::new();
        let context = downcast_ctx(parser.context);
        context.declare_var("a", SyntaxType::Simple(SimpleSyntaxType::Float));

        let input = Input::new_with_str("a := 1");
        let val_type = SyntaxType::Series(SimpleSyntaxType::Float);
        let mut assign = var_assign_with_indent(0)(input, &AstState::new())
            .unwrap()
            .1;
        assert_eq!(
            parser.parse_var_assign(&mut assign),
            Ok(ParseValue::new_with_type(val_type.clone()))
        );

        context.vars.remove("a");
        assert_eq!(
            parser.parse_var_assign(&mut assign),
            Err(PineInputError::new(
                PineErrorKind::VarNotDeclare,
                StrRange::from_start("a := 1", Position::new(0, 0))
            ))
        );
    }
}
