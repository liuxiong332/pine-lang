use super::VarResult;
use crate::ast::syntax_type::{FunctionType, FunctionTypes, SimpleSyntaxType, SyntaxType};
use crate::helper::err_msgs::*;
use crate::helper::str_replace;
use crate::helper::{
    move_element, pine_ref_to_bool, pine_ref_to_color, pine_ref_to_f64, pine_ref_to_i64,
    pine_ref_to_string,
};
use crate::runtime::context::{downcast_ctx, Ctx};
use crate::runtime::output::{OutputData, OutputInfo, PlotInfo, StrOptionsData};
use crate::types::{
    downcast_pf, Bool, Callable, CallableObject, Category, Color, ComplexType, DataType, Float,
    Int, ParamCollectCall, PineClass, PineFrom, PineRef, PineStaticType, PineType, RefData,
    RuntimeErr, SecondType, Series, SeriesCall, SimpleType, NA,
};
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::mem;
use std::rc::Rc;

pub type PerLineItem = Option<PerLine>;

fn pine_ref_to_line<'a>(val: Option<PineRef<'a>>) -> PerLineItem {
    if val.is_none() {
        return None;
    }
    match PerLineItem::implicity_from(val.unwrap()) {
        Ok(res) => res.into_inner(),
        Err(_) => None,
    }
}

// The line definition that represent every line object.
#[derive(Debug, Clone, PartialEq)]
pub struct PerLine {
    x1: Int,
    x2: Int,
}

impl PerLine {
    pub fn new() -> PerLine {
        PerLine { x1: None, x2: None }
    }
}

impl PineStaticType for PerLineItem {
    fn static_type() -> (DataType, SecondType) {
        (DataType::Line, SecondType::Simple)
    }
}

impl<'a> PineFrom<'a, PerLineItem> for PerLineItem {
    fn implicity_from(t: PineRef<'a>) -> Result<RefData<PerLineItem>, RuntimeErr> {
        match t.get_type() {
            (DataType::Line, SecondType::Simple) => Ok(downcast_pf::<PerLineItem>(t).unwrap()),
            (DataType::Line, SecondType::Series) => {
                let f: RefData<Series<PerLineItem>> =
                    downcast_pf::<Series<PerLineItem>>(t).unwrap();
                Ok(RefData::new_box(f.get_current()))
            }
            (DataType::NA, _) => Ok(RefData::new_box(None)),
            _ => Err(RuntimeErr::UnknownRuntimeErr),
        }
    }
}

impl<'a> PineType<'a> for Option<PerLine> {
    fn get_type(&self) -> (DataType, SecondType) {
        (DataType::Line, SecondType::Simple)
    }
    fn copy(&self) -> PineRef<'a> {
        PineRef::new_box(self.clone())
    }
}

impl<'a> SimpleType for Option<PerLine> {}

// // LineInfo represent the series of line object.
// #[derive(Debug, Clone, PartialEq)]
// struct LineInfo<'a> {
//     pub lines: Series<'a, PerLineItem>,
// }

// impl<'a> LineInfo<'a> {
//     fn new_line(&mut self) {
//         self.lines.update(Some(PerLine::new()))
//     }
// }

// impl<'a> PineType<'a> for LineInfo<'a> {
//     fn get_type(&self) -> (DataType, SecondType) {
//         (DataType::Line, SecondType::Simple)
//     }

//     fn category(&self) -> Category {
//         Category::Complex
//     }

//     fn copy(&self) -> PineRef<'a> {
//         PineRef::new_rc(self.clone())
//     }
// }

// impl<'a> ComplexType for LineInfo<'a> {}

// The line invocation that create new LineInfo object
#[derive(Debug)]
struct PlotVal<'a> {
    lines: RefData<Series<'a, PerLineItem>>,
}

impl<'a> Clone for PlotVal<'a> {
    fn clone(&self) -> Self {
        PlotVal {
            lines: RefData::clone(self.lines.borrow()),
        }
    }
}

impl<'a> PlotVal<'a> {
    fn new() -> PlotVal<'a> {
        PlotVal {
            lines: RefData::new(Series::from(None)),
        }
    }
}

impl<'a> SeriesCall<'a> for PlotVal<'a> {
    fn step(
        &mut self,
        _context: &mut dyn Ctx<'a>,
        mut p: Vec<Option<PineRef<'a>>>,
        _func_type: FunctionType<'a>,
    ) -> Result<PineRef<'a>, RuntimeErr> {
        let v = mem::replace(&mut p[0], None);
        let line = pine_ref_to_line(v);
        Ok(PineRef::new_rc(Series::from(line)))
    }

    fn copy(&self) -> Box<dyn SeriesCall<'a> + 'a> {
        Box::new(self.clone())
    }
}

struct PlotProps;

impl<'a> PineClass<'a> for PlotProps {
    fn custom_type(&self) -> &str {
        "line"
    }

    fn get(&self, _ctx: &mut dyn Ctx<'a>, name: &str) -> Result<PineRef<'a>, RuntimeErr> {
        match name {
            _ => Err(RuntimeErr::NotImplement(str_replace(
                NO_FIELD_IN_OBJECT,
                vec![String::from(name), String::from("plot")],
            ))),
        }
    }

    fn copy(&self) -> Box<dyn PineClass<'a> + 'a> {
        Box::new(PlotProps)
    }
}

pub const VAR_NAME: &'static str = "line";

pub fn declare_var<'a>() -> VarResult<'a> {
    let value = PineRef::new(CallableObject::new(Box::new(PlotProps), || {
        Callable::new(None, Some(Box::new(PlotVal::new())))
    }));

    let func_type = FunctionTypes(vec![FunctionType::new((
        vec![("x", SyntaxType::Simple(SimpleSyntaxType::Na))],
        SyntaxType::ObjectClass("line"),
    ))]);
    let mut obj_type = BTreeMap::new();
    obj_type.insert(
        "delete",
        SyntaxType::Function(Rc::new(FunctionTypes(vec![FunctionType::new((
            vec![("id", SyntaxType::ObjectClass("line"))],
            SyntaxType::Void,
        ))]))),
    );
    let syntax_type = SyntaxType::ObjectFunction(Rc::new(obj_type), Rc::new(func_type));
    VarResult::new(value, syntax_type, VAR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{AnySeries, NoneCallback};
    use crate::{LibInfo, PineParser, PineRunner};

    #[test]
    fn line_test() {
        let lib_info = LibInfo::new(
            vec![declare_var()],
            vec![("close", SyntaxType::Series(SimpleSyntaxType::Float))],
        );
        let src = "x = line(na)\nline y = line(na)";
        let blk = PineParser::new(src, &lib_info).parse_blk().unwrap();
        let mut runner = PineRunner::new(&lib_info, &blk, &NoneCallback());

        runner.runl(&vec![], 2, None).unwrap();
    }
}
