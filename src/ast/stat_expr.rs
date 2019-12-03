use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::{map, opt, value},
    multi::{many0, separated_list},
    sequence::{delimited, preceded, terminated, tuple},
};

use super::color::color_lit;
use super::error::PineResult;
use super::func_call::{func_call, func_call_ws};
use super::name::{varname, varname_ws, VarName};
use super::num::{int_lit_ws, num_lit_ws};
use super::op::*;
use super::stat_expr_types::*;
use super::string::string_lit;
use super::trans::flatexp_from_components;
use super::utils::{eat_sep, eat_statement, statement_end, statement_indent};

// exp2 contain the expressions that can apply the binary operators(+,-,*,/) and unary operators(+,-)
pub fn exp2(input: &str) -> PineResult<Exp2> {
    alt((
        value(Exp2::Na, eat_sep(tag("na"))),
        value(Exp2::Bool(true), eat_sep(tag("true"))),
        value(Exp2::Bool(false), eat_sep(tag("false"))),
        map(num_lit_ws, Exp2::Num),
        map(string_lit, Exp2::Str),
        map(color_lit, Exp2::Color),
        map(bracket_expr, Exp2::Exp),
        // map(rettupledef, |varnames| Exp2::RetTuple(Box::new(varnames))), // match [a, b]
        map(tupledef, |exps| Exp2::Tuple(Box::new(exps))), // match [a, b + c]
        map(type_cast, |exp| Exp2::TypeCast(Box::new(exp))), // match float(b)
        map(prefix_exp_ws, |exp| Exp2::PrefixExp(Box::new(exp))), // match a.b.c
        map(func_call_ws, |exp| Exp2::FuncCall(Box::new(exp))), // match a(b)
        map(ref_call, |exp| Exp2::RefCall(Box::new(exp))), // match a[b]
        map(varname_ws, Exp2::VarName),                    // match a
    ))(input)
}

pub fn unopexp2(input: &str) -> PineResult<(Vec<UnaryOp>, Exp2)> {
    tuple((many0(unary_op), exp2))(input)
}

pub fn flatexp(input: &str) -> PineResult<FlatExp> {
    let (input, head) = unopexp2(input)?;
    let (input, binop_chain) = many0(tuple((binary_op, unopexp2)))(input)?;
    Ok((input, flatexp_from_components(head, binop_chain)))
}

pub fn exp(input: &str) -> PineResult<Exp> {
    alt((
        map(condition, |exp| Exp::Condition(Box::new(exp))), // match a ? b : c
        map(flatexp, Exp::from),
    ))(input)
}

// The left return tuple of expression `[a, b] = [1, 2]` that contain variable name between square brackets
fn rettupledef(input: &str) -> PineResult<Vec<VarName>> {
    eat_sep(delimited(
        eat_sep(tag("[")),
        separated_list(eat_sep(tag(",")), varname_ws),
        eat_sep(tag("]")),
    ))(input)
}

// The right tuple of expression `[a, b] = [1, 2]` that contain expressions splited by dot between square brackets
fn tupledef(input: &str) -> PineResult<Vec<Exp>> {
    eat_sep(delimited(
        eat_sep(tag("[")),
        separated_list(eat_sep(tag(",")), exp),
        eat_sep(tag("]")),
    ))(input)
}

fn type_cast(input: &str) -> PineResult<TypeCast> {
    let (input, (data_type, _, e, _)) =
        tuple((eat_sep(datatype), eat_sep(tag("(")), exp, eat_sep(tag(")"))))(input)?;
    Ok((input, TypeCast { data_type, exp: e }))
}

pub fn callable_expr(input: &str) -> PineResult<Exp> {
    alt((
        delimited(tag("("), exp, eat_sep(tag(")"))),
        map(prefix_exp, |exp| Exp::PrefixExp(Box::new(exp))), // match a.b.c
        map(varname, Exp::VarName),                           // match a
    ))(input)
}

fn ref_call(input: &str) -> PineResult<RefCall> {
    let (input, (name, arg)) = tuple((
        eat_sep(callable_expr),
        delimited(eat_sep(tag("[")), exp, eat_sep(tag("]"))),
    ))(input)?;
    Ok((input, RefCall { name, arg }))
}

fn bracket_expr(input: &str) -> PineResult<Exp> {
    delimited(eat_sep(tag("(")), exp, eat_sep(tag(")")))(input)
}

fn condition(input: &str) -> PineResult<Condition> {
    let (input, (cond, _, exp1, _, exp2)) = tuple((
        map(flatexp, |s| Exp::from(s)),
        eat_sep(tag("?")),
        exp,
        eat_sep(tag(":")),
        exp,
    ))(input)?;
    Ok((input, Condition { cond, exp1, exp2 }))
}

fn prefix_exp(input: &str) -> PineResult<PrefixExp> {
    let (input, (prefix, _, names)) =
        tuple((varname, tag("."), separated_list(tag("."), varname)))(input)?;

    Ok((
        input,
        PrefixExp {
            var_chain: [vec![prefix], names].concat(),
        },
    ))
}

fn prefix_exp_ws(input: &str) -> PineResult<PrefixExp> {
    eat_sep(prefix_exp)(input)
}

fn if_then_else<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<IfThenElse> {
    move |input: &'a str| {
        let (input, (_, cond, _, then_block, else_block)) = tuple((
            tag("if"),
            exp,
            statement_end,
            block_with_indent(indent + 1),
            opt(tuple((
                preceded(statement_indent(indent), tag("else")),
                statement_end,
                block_with_indent(indent + 1),
            ))),
        ))(input)?;
        if let Some((_, _, else_block)) = else_block {
            Ok((
                input,
                IfThenElse::new_no_ctxid(cond, then_block, Some(else_block)),
            ))
        } else {
            Ok((input, IfThenElse::new_no_ctxid(cond, then_block, None)))
        }
    }
}

fn if_then_else_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<IfThenElse> {
    move |input: &'a str| preceded(statement_indent(indent), if_then_else(indent))(input)
}

fn for_range<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<ForRange> {
    move |input: &'a str| {
        let (input, (_, var, _, start, _, end, by, _, do_blk)) = tuple((
            tag("for"),
            varname_ws,
            eat_sep(tag("=")),
            exp, // int_lit_ws,
            eat_sep(tag("to")),
            exp, // int_lit_ws,
            opt(tuple((eat_sep(tag("by")), exp))),
            statement_end,
            block_with_indent(indent + 1),
        ))(input)?;
        if let Some((_, step)) = by {
            Ok((
                input,
                ForRange::new_no_ctxid(var, start, end, Some(step), do_blk),
            ))
        } else {
            Ok((input, ForRange::new_no_ctxid(var, start, end, None, do_blk)))
        }
    }
}

fn for_range_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<ForRange> {
    move |input: &'a str| preceded(statement_indent(indent), for_range(indent))(input)
}

fn function_def_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<FunctionDef> {
    move |input: &'a str| {
        let (input, (_, name, _, params, _, _, body)) = tuple((
            statement_indent(indent),
            varname,
            eat_sep(tag("(")),
            separated_list(eat_sep(tag(",")), varname_ws),
            eat_sep(tag(")")),
            eat_sep(tag("=>")),
            alt((
                preceded(statement_end, block_with_indent(indent + 1)),
                map(terminated(exp, statement_end), |s| Block {
                    stmts: vec![],
                    ret_stmt: Some(s),
                }),
            )),
        ))(input)?;
        Ok((input, FunctionDef { name, params, body }))
    }
}

fn datatype(input: &str) -> PineResult<DataType> {
    let (input, label) = alt((
        tag("float"),
        tag("int"),
        tag("bool"),
        tag("color"),
        tag("string"),
        tag("line"),
        tag("label"),
    ))(input)?;
    let data_type = match label {
        "float" => DataType::Float,
        "int" => DataType::Int,
        "bool" => DataType::Bool,
        "color" => DataType::Color,
        "string" => DataType::String,
        "line" => DataType::Line,
        "label" => DataType::Label,
        _ => unreachable!(),
    };
    Ok((input, data_type))
}

pub fn exp_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<Exp> {
    move |input: &'a str| {
        alt((
            terminated(exp, statement_end),
            map(eat_sep(if_then_else(indent)), |s| Exp::Ite(Box::new(s))),
            map(eat_sep(for_range(indent)), |s| Exp::ForRange(Box::new(s))),
        ))(input)
    }
}

fn assign_lv_names<'a>(input: &'a str) -> PineResult<Vec<VarName<'a>>> {
    alt((map(varname_ws, |name| vec![name]), rettupledef))(input)
}

fn assign_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<Assignment> {
    move |input: &'a str| {
        let exp_parser = || exp_with_indent(indent);
        alt((
            map(
                tuple((
                    tag("var"),
                    eat_sep(datatype),
                    assign_lv_names,
                    eat_sep(tag("=")),
                    exp_parser(),
                )),
                |s| Assignment::new(s.2, s.4, true, Some(s.1)),
            ),
            map(
                tuple((tag("var"), assign_lv_names, eat_sep(tag("=")), exp_parser())),
                |s| Assignment::new(s.1, s.3, true, None),
            ),
            map(
                tuple((datatype, assign_lv_names, eat_sep(tag("=")), exp_parser())),
                |s| Assignment::new(s.1, s.3, false, Some(s.0)),
            ),
            map(
                tuple((assign_lv_names, eat_sep(tag("=")), exp_parser())),
                |s| Assignment::new(s.0, s.2, false, None),
            ),
        ))(input)
    }
}

fn var_assign_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<VarAssignment> {
    move |input: &'a str| {
        map(
            tuple((varname, eat_sep(tag(":=")), exp_with_indent(indent))),
            |s| VarAssignment::new(s.0, s.2),
        )(input)
    }
}

fn block_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<Block> {
    move |input: &'a str| {
        let gen_indent = statement_indent(indent);

        let mut stmts: Vec<Statement<'a>> = vec![];
        let mut cur_input = input;
        while cur_input.len() > 0 {
            if let Ok((next_input, stas)) = statement_with_indent(indent)(cur_input) {
                stmts.push(stas);
                cur_input = next_input;
            } else {
                break;
            }
        }
        if cur_input.len() > 0 {
            if let Ok((next_input, ret_stmt)) = eat_statement(gen_indent, exp)(cur_input) {
                return Ok((next_input, Block::new(stmts, Some(ret_stmt))));
            }
        }
        // let block = match stmts.last() {
        //     Some(&Statement::Ite(_)) | Some(&Statement::ForRange(_)) => {
        //         match stmts.pop().unwrap() {
        //             Statement::Ite(s) => Block::new(stmts, Some(Exp::Ite(s))),
        //             Statement::ForRange(s) => Block::new(stmts, Some(Exp::ForRange(s))),
        //             _ => unreachable!(),
        //         }
        //     }
        //     _ => Block::new(stmts, None),
        // };
        Ok((cur_input, Block::new(stmts, None)))
    }
}

fn statement_with_indent<'a>(indent: usize) -> impl Fn(&'a str) -> PineResult<Statement> {
    let gen_indent = statement_indent(indent);
    move |input: &'a str| {
        alt((
            value(Statement::Break, eat_statement(&gen_indent, tag("break"))),
            value(
                Statement::Continue,
                eat_statement(&gen_indent, tag("continue")),
            ),
            map(if_then_else_with_indent(indent), |s| {
                Statement::Ite(Box::new(s))
            }),
            map(for_range_with_indent(indent), |s| {
                Statement::ForRange(Box::new(s))
            }),
            value(Statement::None, statement_end),
            map(function_def_with_indent(indent), |s| {
                Statement::FuncDef(Box::new(s))
            }),
            map(eat_statement(&gen_indent, func_call), |s| {
                Statement::FuncCall(Box::new(s))
            }),
            map(preceded(&gen_indent, assign_with_indent(indent)), |s| {
                Statement::Assignment(Box::new(s))
            }),
            map(preceded(&gen_indent, var_assign_with_indent(indent)), |s| {
                Statement::VarAssignment(Box::new(s))
            }),
        ))(input)
    }
}

pub fn statement(input: &str) -> PineResult<Statement> {
    statement_with_indent(0)(input)
}

pub fn block(input: &str) -> PineResult<Block> {
    block_with_indent(0)(input)
}

#[cfg(test)]
mod tests {
    use super::super::num::Numeral;
    use super::*;

    #[test]
    fn rettupledef_test() {
        assert_eq!(
            rettupledef(" [hello, good]"),
            Ok(("", vec![VarName("hello"), VarName("good")]))
        );
        assert_eq!(
            rettupledef(" [hello, good,  my]hello"),
            Ok((
                "hello",
                vec![VarName("hello"), VarName("good"), VarName("my")]
            ))
        );

        assert_eq!(
            rettupledef(" [ hello  , good ]"),
            Ok(("", vec![VarName("hello"), VarName("good")]))
        );
    }

    #[test]
    fn tupledef_test() {
        assert_eq!(
            tupledef(" [ hello , true ]"),
            Ok(("", vec![Exp::VarName(VarName("hello")), Exp::Bool(true),]))
        );
    }

    #[test]
    fn ref_call_test() {
        assert_eq!(
            ref_call("hello[true]"),
            Ok((
                "",
                RefCall {
                    name: Exp::VarName(VarName("hello")),
                    arg: Exp::Bool(true)
                }
            ))
        );
    }

    #[test]
    fn condition_test() {
        assert_eq!(
            condition("a ? b : c"),
            Ok((
                "",
                Condition {
                    cond: Exp::VarName(VarName("a")),
                    exp1: Exp::VarName(VarName("b")),
                    exp2: Exp::VarName(VarName("c")),
                }
            ))
        );
    }

    #[test]
    fn condition_statement_test() {
        assert_eq!(
            statement_with_indent(0)("m = a ? b : c \n"),
            Ok((
                "",
                Statement::Assignment(Box::new(Assignment::new(
                    vec![VarName("m")],
                    Exp::Condition(Box::new(Condition {
                        cond: Exp::VarName(VarName("a")),
                        exp1: Exp::VarName(VarName("b")),
                        exp2: Exp::VarName(VarName("c")),
                    })),
                    false,
                    None
                )))
            ))
        );
        assert_eq!(
            statement_with_indent(0)("m = na \n"),
            Ok((
                "",
                Statement::Assignment(Box::new(Assignment::new(
                    vec![VarName("m")],
                    Exp::Na,
                    false,
                    None
                )))
            ))
        );
    }

    #[test]
    fn statement_test() {
        assert_eq!(
            statement_with_indent(1)("    break \n"),
            Ok(("", Statement::Break))
        );

        assert_eq!(
            statement_with_indent(0)("a = b \n"),
            Ok((
                "",
                Statement::Assignment(Box::new(Assignment::new(
                    vec![VarName("a")],
                    Exp::VarName(VarName("b")),
                    false,
                    None
                )))
            ))
        );
        assert_eq!(
            statement_with_indent(1)("    a(arg1) \n"),
            Ok((
                "",
                Statement::FuncCall(Box::new(FunctionCall::new_no_ctxid(
                    Exp::VarName(VarName("a")),
                    vec![Exp::VarName(VarName("arg1"))],
                    vec![]
                )))
            ))
        );
        assert_eq!(
            statement_with_indent(1)("    a(arg1) => b \n"),
            Ok((
                "",
                Statement::FuncDef(Box::new(FunctionDef {
                    name: VarName("a"),
                    params: vec![VarName("arg1")],
                    body: Block {
                        stmts: vec![],
                        ret_stmt: Some(Exp::VarName(VarName("b")))
                    }
                }))
            ))
        );

        assert_eq!(
            statement_with_indent(1)("    a(arg1) => \n        b \n"),
            Ok((
                "",
                Statement::FuncDef(Box::new(FunctionDef {
                    name: VarName("a"),
                    params: vec![VarName("arg1")],
                    body: Block {
                        stmts: vec![],
                        ret_stmt: Some(Exp::VarName(VarName("b")))
                    }
                }))
            ))
        );

        assert_eq!(
            statement_with_indent(1)("    //helo world \na = close"),
            Ok(("a = close", Statement::None))
        );
    }

    #[test]
    fn assignment_test() {
        assert_eq!(
            statement_with_indent(0)("a = close // This is also a comment\n"),
            Ok((
                "",
                Statement::Assignment(Box::new(Assignment::new(
                    vec![VarName("a")],
                    Exp::VarName(VarName("close")),
                    false,
                    None
                )))
            ))
        );
        assert_eq!(
            statement_with_indent(0)("a := close\n"),
            Ok((
                "",
                Statement::VarAssignment(Box::new(VarAssignment::new(
                    VarName("a"),
                    Exp::VarName(VarName("close")),
                )))
            ))
        );
    }

    #[test]
    fn prefix_exp_test() {
        assert_eq!(
            statement_with_indent(0)("m = a.b.c \n"),
            Ok((
                "",
                Statement::Assignment(Box::new(Assignment::new(
                    vec![VarName("m")],
                    Exp::PrefixExp(Box::new(PrefixExp {
                        var_chain: vec![VarName("a"), VarName("b"), VarName("c")]
                    })),
                    false,
                    None
                )))
            ))
        )
    }

    #[test]
    fn block_test() {
        assert_eq!(
            block_with_indent(1)("    break \n    continue \n    true \nhello"),
            Ok((
                "hello",
                Block::new(
                    vec![Statement::Break, Statement::Continue],
                    Some(Exp::Bool(true))
                )
            ))
        );
    }

    #[test]
    fn if_then_else_test() {
        assert_eq!(
            if_then_else(0)("if true \n    break\n    true  \n"),
            Ok((
                "",
                IfThenElse::new_no_ctxid(
                    Exp::Bool(true),
                    Block::new(vec![Statement::Break], Some(Exp::Bool(true))),
                    None
                )
            ))
        );
    }

    #[test]
    fn for_range_test() {
        assert_eq!(
            for_range(0)("for a = 1 to 2 \n    break\n    true  \n"),
            Ok((
                "",
                ForRange::new_no_ctxid(
                    VarName("a"),
                    Exp::Num(Numeral::Int(1)),
                    Exp::Num(Numeral::Int(2)),
                    None,
                    Block::new(vec![Statement::Break], Some(Exp::Bool(true))),
                )
            ))
        );
    }
}
