use super::VarResult;
use crate::ast::stat_expr_types::VarIndex;
use crate::ast::syntax_type::{FunctionType, FunctionTypes, SimpleSyntaxType, SyntaxType};
use crate::helper::err_msgs::*;
use crate::helper::str_replace;
use crate::helper::{
    move_element, pine_ref_to_bool, pine_ref_to_f64, pine_ref_to_i64, pine_ref_to_string,
};
use crate::runtime::context::{downcast_ctx, Ctx};
use crate::runtime::output::{
    BoolInputInfo, FloatInputInfo, InputInfo, InputVal, IntInputInfo, SourceInputInfo,
    StringInputInfo,
};
use crate::types::{
    downcast_pf, Bool, Callable, CallableObject, DataType, Float, Int, ParamCollectCall, PineClass,
    PineFrom, PineRef, PineType, RefData, RuntimeErr, SecondType, Series, SeriesCall, Tuple, NA,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

const BOOL_TYPE_STR: &'static str = "bool";
const INT_TYPE_STR: &'static str = "int";
const FLOAT_TYPE_STR: &'static str = "float";
const STRING_TYPE_STR: &'static str = "string";
const SOURCE_TYPE_STR: &'static str = "source";

#[derive(Debug, PartialEq, Clone)]
struct InputCall<'a> {
    val: RefCell<Option<PineRef<'a>>>,
}

impl<'a> InputCall<'a> {
    pub fn new() -> InputCall<'a> {
        InputCall {
            val: RefCell::new(None),
        }
    }
}

impl<'a> SeriesCall<'a> for InputCall<'a> {
    fn step(
        &self,
        context: &mut dyn Ctx<'a>,
        val: Vec<Option<PineRef<'a>>>,
        func_type: FunctionType<'a>,
    ) -> Result<PineRef<'a>, RuntimeErr> {
        if let Some(val) = &*self.val.borrow() {
            return Ok(val.clone());
        }
        match pine_input(context, val, func_type) {
            Err(e) => Err(e),
            Ok(res) => {
                self.val.replace(Some(res.clone()));
                Ok(res)
            }
        }
    }

    fn run(&self, _context: &mut dyn Ctx<'a>) -> Result<(), RuntimeErr> {
        self.val.replace(None);
        Ok(())
    }

    fn copy(&self) -> Box<dyn SeriesCall<'a> + 'a> {
        Box::new(self.clone())
    }
}

fn input_for_bool<'a>(
    context: &mut dyn Ctx<'a>,
    mut param: Vec<Option<PineRef<'a>>>,
) -> Result<PineRef<'a>, RuntimeErr> {
    let ctx_ins = downcast_ctx(context);
    if !ctx_ins.check_is_input_info_ready() {
        let type_str = pine_ref_to_string(move_element(&mut param, 2));

        if type_str.is_some() && type_str.as_ref().unwrap() != BOOL_TYPE_STR {
            // type must be BOOL_TYPE_STR
            return Err(RuntimeErr::FuncCallParamNotValid(str_replace(
                EXP_VAL_BUT_GET_VAL,
                vec![
                    String::from(BOOL_TYPE_STR),
                    String::from(type_str.as_ref().unwrap()),
                ],
            )));
        }
        ctx_ins.push_input_info(InputInfo::Bool(BoolInputInfo {
            defval: pine_ref_to_bool(param[0].clone()),
            title: pine_ref_to_string(move_element(&mut param, 1)),
            input_type: String::from(BOOL_TYPE_STR),
            confirm: pine_ref_to_bool(move_element(&mut param, 3)),
        }));
    }

    let input_val = ctx_ins.copy_next_input();
    match input_val {
        Some(InputVal::Bool(val)) => Ok(PineRef::new_box(val)),
        _ => match move_element(&mut param, 0) {
            Some(val) => Ok(val),
            _ => Err(RuntimeErr::NotValidParam),
        },
    }
}

fn input_for_string<'a>(
    context: &mut dyn Ctx<'a>,
    mut param: Vec<Option<PineRef<'a>>>,
) -> Result<PineRef<'a>, RuntimeErr> {
    let ctx_ins = downcast_ctx(context);
    if !ctx_ins.check_is_input_info_ready() {
        let type_str = pine_ref_to_string(move_element(&mut param, 2));

        if type_str.is_some() && type_str.as_ref().unwrap() != STRING_TYPE_STR {
            // type must be BOOL_TYPE_STR
            return Err(RuntimeErr::FuncCallParamNotValid(str_replace(
                EXP_VAL_BUT_GET_VAL,
                vec![
                    String::from(STRING_TYPE_STR),
                    String::from(type_str.as_ref().unwrap()),
                ],
            )));
        }
        ctx_ins.push_input_info(InputInfo::String(StringInputInfo {
            defval: pine_ref_to_string(param[0].clone()),
            title: pine_ref_to_string(move_element(&mut param, 1)),
            input_type: String::from(STRING_TYPE_STR),
            confirm: pine_ref_to_bool(move_element(&mut param, 3)),
            options: pine_ref_to_str_list(move_element(&mut param, 3)),
        }));
    }

    let input_val = ctx_ins.copy_next_input();
    match input_val {
        Some(InputVal::String(val)) => Ok(PineRef::new_rc(val)),
        _ => match move_element(&mut param, 0) {
            Some(val) => Ok(val),
            _ => Err(RuntimeErr::NotValidParam),
        },
    }
}

const SOURCES: &[&'static str] = &["close", "open", "high", "low"];
fn get_name_from_source<'a>(
    context: &mut dyn Ctx<'a>,
    var: &Option<PineRef<'a>>,
) -> Option<String> {
    if var.is_none() {
        return None;
    }
    SOURCES
        .iter()
        .find(|name| match context.get_varname_index(name) {
            Some(&index) => match context.get_var(VarIndex::new(index, 0)) {
                Some(val) => val.as_ptr() == var.as_ref().unwrap().as_ptr(),
                _ => false,
            },
            _ => false,
        })
        .map(|&s| String::from(s))
}

fn get_source_from_name<'a>(context: &mut dyn Ctx<'a>, name: String) -> Option<PineRef<'a>> {
    match context.get_varname_index(&name) {
        Some(&index) => match context.get_var(VarIndex::new(index, 0)) {
            Some(val) => Some(PineRef::clone(val)),
            _ => None,
        },
        _ => None,
    }
}

fn input_for_source<'a>(
    context: &mut dyn Ctx<'a>,
    mut param: Vec<Option<PineRef<'a>>>,
) -> Result<PineRef<'a>, RuntimeErr> {
    println!("input for source");
    if !downcast_ctx(context).check_is_input_info_ready() {
        let type_str = pine_ref_to_string(move_element(&mut param, 2));

        if type_str.is_some() && type_str.as_ref().unwrap() != SOURCE_TYPE_STR {
            // type must be BOOL_TYPE_STR
            return Err(RuntimeErr::FuncCallParamNotValid(str_replace(
                EXP_VAL_BUT_GET_VAL,
                vec![
                    String::from(SOURCE_TYPE_STR),
                    String::from(type_str.as_ref().unwrap()),
                ],
            )));
        }
        let name = get_name_from_source(context, &param[0]);
        println!("get name {:?}", name);
        downcast_ctx(context).push_input_info(InputInfo::Source(SourceInputInfo {
            defval: name,
            title: pine_ref_to_string(move_element(&mut param, 1)),
            input_type: String::from(SOURCE_TYPE_STR),
        }));
    }

    let input_val = downcast_ctx(context).copy_next_input();
    match input_val {
        Some(InputVal::String(val)) => match get_source_from_name(context, val) {
            Some(pine_val) => Ok(pine_val),
            None => Err(RuntimeErr::NotValidParam),
        },
        _ => match move_element(&mut param, 0) {
            Some(val) => Ok(val),
            _ => Err(RuntimeErr::NotValidParam),
        },
    }
}

pub fn pine_ref_to_list<'a, T, F>(val: Option<PineRef<'a>>, f: F) -> Option<Vec<T>>
where
    F: Fn(Option<PineRef<'a>>) -> Option<T>,
{
    if val.is_none() {
        return None;
    }
    match downcast_pf::<Tuple>(val.unwrap()) {
        Ok(res) => {
            let tuple_val = res.into_inner();
            Some(tuple_val.0.into_iter().filter_map(|s| f(Some(s))).collect())
        }
        Err(_) => None,
    }
}

pub fn pine_ref_to_i64_list<'a>(val: Option<PineRef<'a>>) -> Option<Vec<i64>> {
    pine_ref_to_list(val, pine_ref_to_i64)
}

pub fn pine_ref_to_f64_list<'a>(val: Option<PineRef<'a>>) -> Option<Vec<f64>> {
    pine_ref_to_list(val, pine_ref_to_f64)
}

pub fn pine_ref_to_str_list<'a>(val: Option<PineRef<'a>>) -> Option<Vec<String>> {
    pine_ref_to_list(val, pine_ref_to_string)
}

fn input_for_int<'a>(
    context: &mut dyn Ctx<'a>,
    mut param: Vec<Option<PineRef<'a>>>,
) -> Result<PineRef<'a>, RuntimeErr> {
    move_tuplet!((defval, title, input_type, minval, maxval, confirm, step, options) = param);
    let ctx_ins = downcast_ctx(context);
    if !ctx_ins.check_is_input_info_ready() {
        let type_str = pine_ref_to_string(input_type);
        if type_str.is_some() && type_str.as_ref().unwrap() != INT_TYPE_STR {
            // type must be BOOL_TYPE_STR
            return Err(RuntimeErr::FuncCallParamNotValid(str_replace(
                EXP_VAL_BUT_GET_VAL,
                vec![
                    String::from(INT_TYPE_STR),
                    String::from(type_str.as_ref().unwrap()),
                ],
            )));
        }
        ctx_ins.push_input_info(InputInfo::Int(IntInputInfo {
            defval: pine_ref_to_i64(defval.clone()),
            title: pine_ref_to_string(title),
            input_type: String::from(INT_TYPE_STR),
            minval: pine_ref_to_i64(minval),
            maxval: pine_ref_to_i64(maxval),
            confirm: pine_ref_to_bool(confirm),
            step: pine_ref_to_i64(step),
            options: pine_ref_to_i64_list(options),
        }));
    }

    let input_val = ctx_ins.copy_next_input();
    match input_val {
        Some(InputVal::Int(val)) => Ok(PineRef::new_box(Some(val))),
        _ => match defval {
            Some(val) => Ok(val),
            _ => Err(RuntimeErr::NotValidParam),
        },
    }
}

fn input_for_float<'a>(
    context: &mut dyn Ctx<'a>,
    mut param: Vec<Option<PineRef<'a>>>,
) -> Result<PineRef<'a>, RuntimeErr> {
    move_tuplet!((defval, title, input_type, minval, maxval, confirm, step, options) = param);
    let ctx_ins = downcast_ctx(context);
    if !ctx_ins.check_is_input_info_ready() {
        let type_str = pine_ref_to_string(input_type);
        if type_str.is_some() && type_str.as_ref().unwrap() != FLOAT_TYPE_STR {
            // type must be BOOL_TYPE_STR
            return Err(RuntimeErr::FuncCallParamNotValid(str_replace(
                EXP_VAL_BUT_GET_VAL,
                vec![
                    String::from(FLOAT_TYPE_STR),
                    String::from(type_str.as_ref().unwrap()),
                ],
            )));
        }
        ctx_ins.push_input_info(InputInfo::Float(FloatInputInfo {
            defval: pine_ref_to_f64(defval.clone()),
            title: pine_ref_to_string(title),
            input_type: String::from(FLOAT_TYPE_STR),
            minval: pine_ref_to_f64(minval),
            maxval: pine_ref_to_f64(maxval),
            confirm: pine_ref_to_bool(confirm),
            step: pine_ref_to_f64(step),
            options: pine_ref_to_f64_list(options),
        }));
    }

    let input_val = ctx_ins.copy_next_input();
    match input_val {
        Some(InputVal::Int(val)) => Ok(PineRef::new_box(Some(val))),
        _ => match defval {
            Some(val) => Ok(val),
            _ => Err(RuntimeErr::NotValidParam),
        },
    }
}

fn gen_bool_type<'a>() -> FunctionType<'a> {
    FunctionType::new((
        vec![
            ("defval", SyntaxType::bool()),
            ("title", SyntaxType::string()),
            ("type", SyntaxType::string()),
            ("confirm", SyntaxType::bool()),
        ],
        SyntaxType::bool(),
    ))
}

fn gen_int_type<'a>() -> FunctionType<'a> {
    FunctionType::new((
        vec![
            ("defval", SyntaxType::int()),
            ("title", SyntaxType::string()),
            ("type", SyntaxType::string()),
            ("minval", SyntaxType::int()),
            ("maxval", SyntaxType::int()),
            ("confirm", SyntaxType::bool()),
            ("step", SyntaxType::int()),
            ("options", SyntaxType::List(SimpleSyntaxType::Int)),
        ],
        SyntaxType::int(),
    ))
}

fn gen_float_type<'a>() -> FunctionType<'a> {
    FunctionType::new((
        vec![
            ("defval", SyntaxType::float()),
            ("title", SyntaxType::string()),
            ("type", SyntaxType::string()),
            ("minval", SyntaxType::float()),
            ("maxval", SyntaxType::float()),
            ("confirm", SyntaxType::bool()),
            ("step", SyntaxType::float()),
            ("options", SyntaxType::List(SimpleSyntaxType::Float)),
        ],
        SyntaxType::float(),
    ))
}

fn gen_string_type<'a>() -> FunctionType<'a> {
    FunctionType::new((
        vec![
            ("defval", SyntaxType::string()),
            ("title", SyntaxType::string()),
            ("type", SyntaxType::string()),
            ("confirm", SyntaxType::bool()),
            ("options", SyntaxType::List(SimpleSyntaxType::String)),
        ],
        SyntaxType::string(),
    ))
}

fn gen_source_type<'a>() -> FunctionType<'a> {
    FunctionType::new((
        vec![
            ("defval", SyntaxType::Series(SimpleSyntaxType::Float)),
            ("title", SyntaxType::string()),
            ("type", SyntaxType::string()),
        ],
        SyntaxType::Series(SimpleSyntaxType::Float),
    ))
}

fn pine_input<'a>(
    context: &mut dyn Ctx<'a>,
    param: Vec<Option<PineRef<'a>>>,
    func_type: FunctionType<'a>,
) -> Result<PineRef<'a>, RuntimeErr> {
    println!("func type {:?}", func_type);
    if func_type.arg_names().len() == 4 {
        input_for_bool(context, param)
    } else if func_type.arg_names().len() == 8 {
        if func_type == gen_int_type() {
            input_for_int(context, param)
        } else if func_type == gen_float_type() {
            input_for_float(context, param)
        } else {
            unreachable!();
        }
    } else if func_type.arg_names().len() == 5 {
        input_for_string(context, param)
    } else if func_type.arg_names().len() == 3 {
        input_for_source(context, param)
    } else {
        unreachable!();
    }
}

struct InputProps;

impl<'a> PineClass<'a> for InputProps {
    fn custom_type(&self) -> &str {
        "input"
    }

    fn get(&self, name: &str) -> Result<PineRef<'a>, RuntimeErr> {
        match name {
            "bool" => Ok(PineRef::new_rc(String::from(BOOL_TYPE_STR))),
            "float" => Ok(PineRef::new_rc(String::from(FLOAT_TYPE_STR))),
            "integer" => Ok(PineRef::new_rc(String::from(INT_TYPE_STR))),
            "resolution" => Ok(PineRef::new_rc(String::from(STRING_TYPE_STR))),
            "session" => Ok(PineRef::new_rc(String::from(STRING_TYPE_STR))),
            "source" => Ok(PineRef::new_rc(String::from(SOURCE_TYPE_STR))),
            "string" => Ok(PineRef::new_rc(String::from(STRING_TYPE_STR))),
            "symbol" => Ok(PineRef::new_rc(String::from(STRING_TYPE_STR))),
            _ => Err(RuntimeErr::NotImplement(str_replace(
                NO_FIELD_IN_OBJECT,
                vec![String::from(name), String::from("input")],
            ))),
        }
    }

    fn copy(&self) -> Box<dyn PineClass<'a> + 'a> {
        Box::new(InputProps)
    }
}

pub const VAR_NAME: &'static str = "input";

pub fn declare_var<'a>() -> VarResult<'a> {
    let value = PineRef::new(CallableObject::new(Box::new(InputProps), || {
        Callable::new(None, Some(Box::new(InputCall::new())))
    }));
    /*
        input(defval, title, type, confirm) → input bool
        input(defval, title, type, minval, maxval, confirm, step, options) → input integer
        input(defval, title, type, minval, maxval, confirm, step, options) → input float
        input(defval, title, type, confirm, options) → input string
        input(defval, title, type) → series[float]
    */
    let mut obj_type = BTreeMap::new();
    obj_type.insert("bool", SyntaxType::string());
    obj_type.insert("float", SyntaxType::string());
    obj_type.insert("integer", SyntaxType::string());
    obj_type.insert("resolution", SyntaxType::string());
    obj_type.insert("session", SyntaxType::string());
    obj_type.insert("source", SyntaxType::string());
    obj_type.insert("string", SyntaxType::string());
    obj_type.insert("symbol", SyntaxType::string());
    let syntax_type = SyntaxType::ObjectFunction(
        Rc::new(obj_type),
        Rc::new(FunctionTypes(vec![
            gen_int_type(),
            gen_float_type(),
            gen_bool_type(),
            gen_string_type(),
            gen_source_type(),
        ])),
    );
    VarResult::new(value, syntax_type, VAR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::stat_expr_types::VarIndex;
    use crate::runtime::context::VarOperate;
    use crate::runtime::data_src::NoneCallback;
    use crate::types::PineRef;
    use crate::{LibInfo, PineParser, PineRunner};

    #[test]
    fn input_type_test() {
        let lib_info = LibInfo::new(vec![declare_var()], vec![]);
        PineParser::new("input(true, 'title', 'bool', false)", &lib_info);
        PineParser::new(
            "input(defval = true, title = 'title', type = 'bool', confirm = false)",
            &lib_info,
        );
        PineParser::new(
            "input(true, 'hello', defval = true, title = 'title', type = 'bool', confirm = false)",
            &lib_info,
        );

        PineParser::new(
            "input(1, 'hello', type = 'int', confirm = false)",
            &lib_info,
        );

        PineParser::new(
            "input(1, 'hello', 'int', 1, 10, true, 1, [1, 2, 3])",
            &lib_info,
        );

        PineParser::new(
            "input(1, 'hello', 'float', 1, 10, true, 1, [1, 2, 3])",
            &lib_info,
        );

        PineParser::new("input(1, 'hello', 'int')", &lib_info);
    }

    #[test]
    fn bool_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "m = input(true, 'title', 'bool', false)";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(true))
        );

        runner.change_inputs(vec![Some(InputVal::Bool(false))]);
        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(false))
        );
        assert_eq!(
            runner.get_context().get_io_info().get_inputs(),
            &vec![InputInfo::Bool(BoolInputInfo {
                defval: Some(true),
                title: Some(String::from("title")),
                input_type: String::from("bool"),
                confirm: Some(false),
            })]
        )
    }

    #[test]
    fn int_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "m = input(1, 'hello', 'int', 1, 10, true, 1, [1, 2, 3])";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(Some(1)))
        );

        runner.change_inputs(vec![Some(InputVal::Int(4))]);
        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(Some(4)))
        );
        assert_eq!(
            runner.get_context().get_io_info().get_inputs(),
            &vec![InputInfo::Int(IntInputInfo {
                defval: Some(1),
                title: Some(String::from("hello")),
                input_type: String::from("int"),
                confirm: Some(true),
                minval: Some(1),
                maxval: Some(10),
                step: Some(1),
                options: Some(vec![1, 2, 3])
            })]
        )
    }

    #[test]
    fn some_int_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "m = input(1, 'hello', 'int')";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());
        runner.change_inputs(vec![Some(InputVal::Int(4))]);
        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(Some(4)))
        );
    }

    #[test]
    fn float_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "m = input(1.5, 'hello', 'float', 1, 10, true, 1, [1, 2, 3])";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_box(Some(1.5f64)))
        );
    }

    #[test]
    fn string_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "m = input('defval', 'hello', 'string')";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(2, 0)),
            Some(PineRef::new_rc(String::from("defval")))
        );
    }

    #[test]
    fn source_input_test<'a>() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![
                ("close", SyntaxType::Series(SimpleSyntaxType::Float)),
                ("open", SyntaxType::Series(SimpleSyntaxType::Float)),
            ],
        );
        let src = "m = input(close, 'hello', 'source')";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner
            .run(&vec![
                ("close", vec![Some(1f64)]),
                ("open", vec![Some(2f64)]),
            ])
            .unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(3, 0)),
            Some(PineRef::new_rc(Series::from_vec(vec![Some(1f64)])))
        );

        runner.change_inputs(vec![Some(InputVal::String(String::from("open")))]);
        runner.run(&vec![("open", vec![Some(10f64)])]).unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(3, 0)),
            Some(PineRef::new_rc(Series::from_vec(vec![Some(10f64)])))
        );
    }

    #[test]
    fn input_fields_test() {
        use crate::types::Tuple;

        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::float_series())],
        );
        let src = r"m = [
            input.bool, input.float, input.integer, input.resolution, 
            input.session, input.source, input.string, input.symbol
        ]";

        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.run(&vec![("close", vec![Some(1f64)])]).unwrap();
        let tuple_res =
            downcast_pf::<Tuple>(runner.get_context().move_var(VarIndex::new(2, 0)).unwrap());
        assert_eq!(
            tuple_res.unwrap().into_inner(),
            Tuple(vec![
                PineRef::new_rc(String::from(BOOL_TYPE_STR)),
                PineRef::new_rc(String::from(FLOAT_TYPE_STR)),
                PineRef::new_rc(String::from(INT_TYPE_STR)),
                PineRef::new_rc(String::from(STRING_TYPE_STR)),
                PineRef::new_rc(String::from(STRING_TYPE_STR)),
                PineRef::new_rc(String::from(SOURCE_TYPE_STR)),
                PineRef::new_rc(String::from(STRING_TYPE_STR)),
                PineRef::new_rc(String::from(STRING_TYPE_STR)),
            ])
        );
    }
}
