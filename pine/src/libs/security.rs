use super::VarResult;
use crate::ast::input::StrRange;
use crate::ast::stat_expr_types::VarIndex;
use crate::ast::syntax_type::{FunctionType, FunctionTypes, SimpleSyntaxType, SyntaxType};
use crate::helper::err_msgs::*;
use crate::helper::str_replace;
use crate::helper::{
    move_element, pine_ref_to_bool, pine_ref_to_color, pine_ref_to_f64, pine_ref_to_i64,
    pine_ref_to_string,
};
use crate::runtime::context::{downcast_ctx, Context, ContextType, Ctx, VarOperate};
use crate::runtime::function::Function;
use crate::runtime::output::InputSrc;
use crate::runtime::{AnySeries, AnySeriesType};
use crate::types::{
    downcast_pf, Callable, CallableFactory, Color, DataType, Float, Int, PineFrom, PineRef,
    RefData, RuntimeErr, Series, SeriesCall, NA,
};
use std::cell::RefCell;
use std::mem;
use std::mem::ManuallyDrop;
use std::rc::Rc;

struct SecurityInfo<'a> {
    ticker: Option<String>,
    ctx: Option<Box<dyn Ctx<'a>>>,
    fun_def: Option<RefData<Function<'a>>>,
    time_index: Option<VarIndex>,
    start_time_data_index: isize,
    data_names: Vec<String>,
    last_result: Option<PineRef<'a>>,
}

fn gen_ticker<'a>(
    symbol: Option<PineRef<'a>>,
    resolution: Option<PineRef<'a>>,
) -> Result<String, RuntimeErr> {
    match (pine_ref_to_string(symbol), pine_ref_to_string(resolution)) {
        (Some(s), Some(r)) => Ok(format!("{}-{}", s, r)),
        _ => Err(RuntimeErr::InvalidParameters(str_replace(
            REQUIRED_PARAMETERS,
            vec![String::from("symbol, resolution")],
        ))),
    }
}

fn get_func<'a>(func: Option<PineRef<'a>>) -> Result<RefData<Function<'a>>, RuntimeErr> {
    match func {
        None => Err(RuntimeErr::InvalidParameters(str_replace(
            REQUIRED_PARAMETERS,
            vec![String::from("expression")],
        ))),
        Some(fun) => Ok(downcast_pf::<Function>(fun).unwrap()),
    }
}

impl<'a> SecurityInfo<'a> {
    pub fn new() -> SecurityInfo<'a> {
        SecurityInfo {
            ticker: None,
            ctx: None,
            fun_def: None,
            time_index: None,
            start_time_data_index: 0,
            data_names: vec![],
            last_result: None,
        }
    }

    fn init_input_info(
        &mut self,
        _context: &mut dyn Ctx<'a>,
        symbol: Option<PineRef<'a>>,
        resolution: Option<PineRef<'a>>,
        expression: Option<PineRef<'a>>,
    ) -> Result<(), RuntimeErr> {
        self.ticker = Some(gen_ticker(symbol, resolution)?);
        let func_ins = get_func(expression)?;
        let var_i = *downcast_ctx(_context.get_top_ctx())
            .get_varname_index("_time")
            .unwrap();
        self.time_index = Some(VarIndex::new(var_i, 0));

        self.fun_def = Some(func_ins);
        let params = &self.fun_def.as_ref().unwrap().get_def().params;
        let mut names: Vec<_> = params.iter().map(|s| String::from(s.value)).collect();
        // The new ticker must contain time field.
        if !names.contains(&String::from("time")) {
            names.push(String::from("time"));
        }
        // Add the new ticker information to input sources.
        downcast_ctx(_context).add_input_src(InputSrc::new(self.ticker.clone(), names));
        Ok(())
    }

    pub fn init_subctx(&mut self, _context: &mut dyn Ctx<'a>) {
        let mut subctx = Box::new(Context::new(Some(_context), ContextType::FuncDefBlock));
        let func_ins = self.fun_def.as_ref().unwrap();
        subctx.init(
            func_ins.get_var_count(),
            func_ins.get_subctx_count(),
            func_ins.get_libfun_count(),
        );
        let params = &func_ins.get_def().params;
        let names: Vec<_> = params.iter().map(|s| s.value).collect();

        let ticker = self.ticker.as_ref().unwrap();
        self.data_names = names
            .iter()
            .map(|name| format!("{}-{}", ticker, name))
            .collect();

        let s: Box<dyn Ctx<'a>> = subctx;
        let ctx = unsafe { mem::transmute::<Box<dyn Ctx<'a>>, Box<dyn Ctx<'a>>>(s) };
        self.ctx = Some(ctx);
    }
}

fn find_nearest_index(data: &ManuallyDrop<Vec<Int>>, val: &Int, is_ge: bool) -> isize {
    match data.binary_search(val) {
        Ok(index) => index as isize,
        Err(index) => {
            if is_ge {
                index as isize
            } else {
                index as isize - 1
            }
        }
    }
}

impl<'a> SeriesCall<'a> for SecurityInfo<'a> {
    fn step(
        &mut self,
        _context: &mut dyn Ctx<'a>,
        mut param: Vec<Option<PineRef<'a>>>,
        _func_type: FunctionType<'a>,
    ) -> Result<PineRef<'a>, RuntimeErr> {
        move_tuplet!((symbol, resolution, expression, gaps, lookahead) = param);

        if !downcast_ctx(_context).check_is_input_info_ready() {
            self.init_input_info(_context, symbol, resolution, expression)?;
        }
        // Crate the sub context to run the function.
        if self.ctx.is_none() {
            self.init_subctx(_context);
        }
        let gaps = pine_ref_to_bool(gaps).unwrap_or(false);
        let lookahead = pine_ref_to_bool(lookahead).unwrap_or(false);
        // let func_ins = self.fun_def.as_ref().unwrap();

        let subctx = &mut **self.ctx.as_mut().unwrap();
        let time = pine_ref_to_i64(
            subctx
                .get_top_ctx()
                .get_var(self.time_index.clone().unwrap())
                .clone(),
        );
        match time {
            None => Ok(PineRef::new_box(NA)),
            Some(cur_time) => {
                match downcast_ctx(subctx)
                    .get_input_data(&format!("{}-_time", self.ticker.as_ref().unwrap()))
                {
                    Some(series) => {
                        let time_data = series.as_vec::<Int>();
                        // If the lookahead is false, we will find the point that the time is equal or less thant current time.
                        // else if the lookahead is true, we will find the point that the time is equal or greater than current time.
                        let end_index =
                            find_nearest_index(&time_data, &Some(cur_time), lookahead) + 1;

                        // Will run the data in the range start_time_data_index..end_index
                        if end_index > self.start_time_data_index {
                            let mut results: Vec<PineRef<'a>> = vec![];
                            for i in self.start_time_data_index..end_index {
                                let mut input_params: Vec<PineRef<'a>> = vec![];

                                for data_name in &self.data_names {
                                    match downcast_ctx(subctx).get_input_data(data_name) {
                                        None => {
                                            input_params.push(PineRef::new_rc(Series::from(
                                                Int::from(None),
                                            )));
                                        }
                                        Some(origin_data) => match origin_data.get_type() {
                                            AnySeriesType::Int => {
                                                input_params.push(PineRef::new_rc(Series::from(
                                                    origin_data.index::<Int>(i as isize),
                                                )));
                                            }
                                            AnySeriesType::Float => {
                                                input_params.push(PineRef::new_rc(Series::from(
                                                    origin_data.index::<Float>(i as isize),
                                                )));
                                            }
                                        },
                                    }
                                }

                                // TODO: bar_index need be handled specially.
                                let result = self.fun_def.as_mut().unwrap().call(
                                    subctx,
                                    input_params,
                                    vec![],
                                    StrRange::new_empty(),
                                );
                                downcast_ctx(subctx).commit();
                                match result {
                                    Ok(val) => {
                                        results.push(val);
                                    }
                                    Err(e) => return Err(e.code),
                                }
                            }
                            self.start_time_data_index = end_index;
                            self.last_result = Some(results.last().unwrap().clone());
                            if lookahead {
                                return Ok(results.last().unwrap().clone());
                            } else {
                                return Ok(results[0].clone());
                            }
                        } else {
                            if self.last_result.is_none() {
                                match _func_type.signature.1 {
                                    SyntaxType::Series(SimpleSyntaxType::Int) => {
                                        self.last_result =
                                            Some(PineRef::new_rc(Series::from(Int::from(None))));
                                    }
                                    SyntaxType::Series(SimpleSyntaxType::Float) => {
                                        self.last_result =
                                            Some(PineRef::new_rc(Series::from(Float::from(None))));
                                    }
                                    SyntaxType::Series(SimpleSyntaxType::Bool) => {
                                        self.last_result =
                                            Some(PineRef::new_rc(Series::from(false)));
                                    }
                                    SyntaxType::Series(SimpleSyntaxType::Color) => {
                                        self.last_result =
                                            Some(PineRef::new_rc(Series::from(Color(""))));
                                    }
                                    SyntaxType::Series(SimpleSyntaxType::String) => {
                                        self.last_result =
                                            Some(PineRef::new_rc(Series::from(String::from(""))));
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            Ok(self.last_result.as_ref().unwrap().clone())
                        }
                    }
                    None => Ok(PineRef::new_box(NA)),
                }
            }
        }
    }

    fn copy(&self) -> Box<dyn SeriesCall<'a> + 'a> {
        Box::new(SecurityInfo {
            ticker: self.ticker.clone(),
            ctx: None,
            fun_def: None,
            time_index: None,
            start_time_data_index: 0,
            data_names: vec![],
            last_result: None,
        })
    }
}

pub const VAR_NAME: &'static str = "security";

pub fn declare_var<'a>() -> VarResult<'a> {
    let value = PineRef::new(CallableFactory::new(|| {
        Callable::new(None, Some(Box::new(SecurityInfo::new())))
    }));
    let func_type = FunctionTypes(vec![FunctionType::new((
        vec![
            ("symbol", SyntaxType::string()),
            ("resolution", SyntaxType::string()),
            (
                "expression",
                SyntaxType::DynamicExpr(Box::new(SyntaxType::float_series())),
            ),
            ("gaps", SyntaxType::color()),
            ("lookahead", SyntaxType::bool()),
        ],
        SyntaxType::float_series(),
    ))]);
    let syntax_type = SyntaxType::Function(Rc::new(func_type));
    VarResult::new(value, syntax_type, VAR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::stat_expr_types::VarIndex;
    use crate::runtime::context::VarOperate;
    use crate::runtime::{AnySeries, NoneCallback};
    use crate::{LibInfo, PineParser, PineRunner};

    #[test]
    fn security_test() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![
                ("close", SyntaxType::Series(SimpleSyntaxType::Float)),
                ("_time", SyntaxType::Series(SimpleSyntaxType::Int)),
            ],
        );
        let src = "a = close + 1\nm = security('MSFT', '1D', close + a)";
        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner
            .run(
                &vec![
                    (
                        "close",
                        AnySeries::from_float_vec(vec![Some(1f64), Some(2f64)]),
                    ),
                    (
                        "_time",
                        AnySeries::from_int_vec(vec![Some(10i64), Some(20i64)]),
                    ),
                    ("MSFT-1D-_time", AnySeries::from_int_vec(vec![Some(15i64)])),
                    (
                        "MSFT-1D-close",
                        AnySeries::from_float_vec(vec![Some(15f64)]),
                    ),
                ],
                None,
            )
            .unwrap();
        assert_eq!(
            runner.get_context().move_var(VarIndex::new(5, 0)),
            Some(PineRef::new_rc(Series::from_vec(vec![None, Some(31f64)])))
        );
    }
}
