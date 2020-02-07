use super::data_src::Callback;
use super::output::InputVal;
use super::output::{IOInfo, InputInfo, OutputData, OutputInfo};
use crate::ast::input::{Position, StrRange};
use crate::ast::stat_expr_types::VarIndex;
use crate::types::{
    Bool, Callable, Color, DataType, Float, Int, PineFrom, PineRef, PineStaticType, PineType,
    RefData, RuntimeErr, SecondType, Series, NA,
};
use std::collections::HashSet;
use std::fmt::Debug;
use std::mem;

pub trait VarOperate<'a> {
    fn create_var(&mut self, index: i32, val: PineRef<'a>) -> Option<PineRef<'a>>;

    fn update_var(&mut self, index: VarIndex, val: PineRef<'a>);

    fn move_var(&mut self, index: VarIndex) -> Option<PineRef<'a>>;

    fn get_var(&self, index: VarIndex) -> &Option<PineRef<'a>>;

    fn var_len(&self) -> i32;
}

// lifetime 'a is the lifetime of Exp, 'c is the lifetime of Ctx Self's lifetime
pub trait Ctx<'a>: VarOperate<'a> {
    fn contains_var(&self, index: VarIndex) -> bool;

    fn contains_var_scope(&self, index: i32) -> bool;

    fn create_callable(&mut self, call: RefData<Callable<'a>>);

    fn move_fun_instance(&mut self, index: i32) -> Option<RefData<Callable<'a>>>;

    fn create_fun_instance(&mut self, index: i32, val: RefData<Callable<'a>>);

    // fn create_declare(&mut self, name: &'a str);

    // fn contains_declare(&self, name: &'a str) -> bool;

    // fn contains_declare_scope(&self, name: &'a str) -> bool;

    // fn clear_declare(&mut self);

    // fn any_declare(&self) -> bool;

    fn set_is_run(&mut self, is_run: bool);

    fn get_is_run(&self) -> bool;

    fn clear_is_run(&mut self);

    fn get_type(&self) -> ContextType;

    fn get_callback(&self) -> Option<&'a dyn Callback>;
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ContextType {
    Normal,
    IfElseBlock,
    ForRangeBlock,
    FuncDefBlock,
}

// 'a is the lifetime of Exp, 'b is the parent context's lifetime, 'c is the context self's lifetime
pub struct Context<'a, 'b, 'c> {
    // input: &'a str,
    parent: Option<&'b mut (dyn 'b + Ctx<'a>)>,
    context_type: ContextType,

    // Child contexts that with parent self lifetime 'c
    sub_contexts: Vec<Option<Box<dyn 'c + Ctx<'a>>>>,

    // variable map that defined by user and library.
    vars: Vec<Option<PineRef<'a>>>,

    // function instances
    fun_instances: Vec<Option<RefData<Callable<'a>>>>,

    // All the Series type variable name
    _series: Vec<&'a str>,
    callables: Vec<RefData<Callable<'a>>>,
    // declare_vars: HashSet<&'a str>,

    // The input value from user
    inputs: Vec<Option<InputVal>>,
    // The input index will increment after input function is invoked
    input_index: i32,

    outputs: Vec<Option<OutputData>>,

    io_info: IOInfo,
    // Check if input_info is ready
    is_input_info_ready: bool,
    // Check if input_info is ready
    is_output_info_ready: bool,

    // The range of data
    data_range: (Option<i32>, Option<i32>),

    // The output values
    callback: Option<&'a dyn Callback>,
    first_commit: bool,

    is_run: bool,
}

pub fn downcast_ctx<'a, 'b, 'c>(item: &'c mut (dyn Ctx<'a> + 'c)) -> &'c mut Context<'a, 'b, 'c> {
    unsafe {
        let raw: *mut dyn Ctx<'a> = item;
        let t = raw as *mut Context<'a, 'b, 'c>;
        t.as_mut().unwrap()
    }
}

pub fn downcast_ctx_const<'a, 'b, 'c>(item: &'c (dyn Ctx<'a> + 'c)) -> &'c Context<'a, 'b, 'c> {
    unsafe {
        let raw: *const dyn Ctx<'a> = item;
        let t = raw as *const Context<'a, 'b, 'c>;
        t.as_ref().unwrap()
    }
}

fn commit_series<'a, D>(val: PineRef<'a>) -> PineRef<'a>
where
    D: Default + PartialEq + PineStaticType + PineType<'a> + PineFrom<'a, D> + Clone + Debug + 'a,
{
    let mut series: RefData<Series<D>> = Series::implicity_from(val).unwrap();
    series.commit();
    series.into_pf()
}

pub fn commit_series_for_operator<'a>(operator: &mut dyn VarOperate<'a>) {
    let len: i32 = operator.var_len();
    // The committed set used to make sure only one instance of series commmit.
    let mut commited: HashSet<*const (dyn PineType<'a> + 'a)> = HashSet::new();
    for k in 0..len {
        let index = VarIndex::new(k, 0);
        if let Some(val) = operator.move_var(index) {
            if commited.contains(&val.as_ptr()) {
                continue;
            }
            commited.insert(val.as_ptr());
            let ret_val = match val.get_type() {
                (DataType::Float, SecondType::Series) => commit_series::<Float>(val),
                (DataType::Int, SecondType::Series) => commit_series::<Int>(val),
                (DataType::Color, SecondType::Series) => commit_series::<Color>(val),
                (DataType::Bool, SecondType::Series) => commit_series::<Bool>(val),
                _ => val,
            };
            operator.update_var(index, ret_val);
        }
    }
}

fn roll_back_series<'a, D>(val: PineRef<'a>) -> PineRef<'a>
where
    D: Default + PartialEq + PineStaticType + PineType<'a> + PineFrom<'a, D> + Clone + Debug + 'a,
{
    let mut series: RefData<Series<D>> = Series::implicity_from(val).unwrap();
    series.roll_back();
    series.into_pf()
}

impl<'a, 'b, 'c> Context<'a, 'b, 'c> {
    pub fn new(parent: Option<&'b mut (dyn 'b + Ctx<'a>)>, t: ContextType) -> Context<'a, 'b, 'c> {
        Context {
            parent,
            context_type: t,
            sub_contexts: Vec::new(),
            vars: Vec::new(),
            fun_instances: Vec::new(),
            _series: vec![],
            callables: vec![],
            // declare_vars: HashSet::new(),
            callback: None,
            inputs: vec![],
            input_index: -1,
            outputs: vec![],
            io_info: IOInfo::new(),
            is_input_info_ready: false,
            is_output_info_ready: false,
            data_range: (Some(0), Some(0)),
            first_commit: false,
            is_run: false,
        }
    }

    pub fn new_with_callback(callback: &'a dyn Callback) -> Context<'a, 'b, 'c> {
        Context {
            parent: None,
            context_type: ContextType::Normal,
            sub_contexts: Vec::new(),
            vars: Vec::new(),
            fun_instances: Vec::new(),
            _series: vec![],
            callables: vec![],
            // declare_vars: HashSet::new(),
            callback: Some(callback),
            inputs: vec![],
            input_index: -1,
            outputs: vec![],
            io_info: IOInfo::new(),
            is_input_info_ready: false,
            is_output_info_ready: false,
            data_range: (Some(0), Some(0)),
            first_commit: false,
            is_run: false,
        }
    }

    pub fn init_vars(&mut self, vars: Vec<Option<PineRef<'a>>>) {
        self.vars = vars;
    }

    pub fn init_sub_contexts(&mut self, sub_contexts: Vec<Option<Box<dyn 'c + Ctx<'a>>>>) {
        self.sub_contexts = sub_contexts;
    }

    pub fn init_fun_instances(&mut self, fun_instances: Vec<Option<RefData<Callable<'a>>>>) {
        self.fun_instances = fun_instances;
    }

    pub fn init(&mut self, var_count: i32, subctx_count: i32, libfun_count: i32) {
        let mut vars: Vec<Option<PineRef<'a>>> = Vec::with_capacity(var_count as usize);
        vars.resize(var_count as usize, None);
        self.init_vars(vars);

        let ctx_count = subctx_count as usize;
        let mut ctxs: Vec<Option<Box<dyn 'c + Ctx<'a>>>> = Vec::with_capacity(ctx_count);
        ctxs.resize_with(ctx_count, || None);
        self.init_sub_contexts(ctxs);

        let fun_count = libfun_count as usize;
        let mut funs: Vec<Option<RefData<Callable<'a>>>> = Vec::with_capacity(fun_count);
        funs.resize_with(fun_count, || None);
        self.init_fun_instances(funs);
    }

    pub fn change_inputs(&mut self, inputs: Vec<Option<InputVal>>) {
        debug_assert!(
            self.io_info.get_inputs().is_empty() || inputs.len() == self.io_info.get_inputs().len()
        );
        self.inputs = inputs;
    }

    pub fn get_inputs(&self) -> &Vec<Option<InputVal>> {
        &self.inputs
    }

    pub fn copy_next_input(&mut self) -> Option<InputVal> {
        self.input_index += 1;
        if self.input_index as usize >= self.inputs.len() {
            None
        } else {
            self.inputs[self.input_index as usize].clone()
        }
    }

    pub fn get_next_input_index(&mut self) -> i32 {
        self.input_index += 1;
        self.input_index
    }

    pub fn reset_input_index(&mut self) {
        self.input_index = -1;
    }

    // io_info related methods
    pub fn push_input_info(&mut self, input: InputInfo) {
        self.io_info.push_input(input);
    }

    pub fn push_output_info(&mut self, output: OutputInfo) {
        self.io_info.push_output(output);
    }

    pub fn get_io_info(&self) -> &IOInfo {
        &self.io_info
    }

    pub fn check_is_input_info_ready(&self) -> bool {
        self.is_input_info_ready
    }

    pub fn let_input_info_ready(&mut self) {
        self.is_input_info_ready = true;
    }

    pub fn check_is_output_info_ready(&self) -> bool {
        self.is_output_info_ready
    }

    pub fn let_output_info_ready(&mut self) {
        self.is_output_info_ready = true;
    }

    pub fn push_output_data(&mut self, data: Option<OutputData>) {
        self.outputs.push(data);
    }

    pub fn move_output_data(&mut self) -> Vec<Option<OutputData>> {
        println!(
            "data len {:?} {:?}",
            self.outputs.len(),
            self.io_info.get_outputs().len()
        );
        debug_assert_eq!(self.outputs.len(), self.io_info.get_outputs().len());
        mem::replace(&mut self.outputs, vec![])
    }

    pub fn get_data_range(&self) -> (Option<i32>, Option<i32>) {
        self.data_range.clone()
    }

    pub fn update_data_range(&mut self, range: (Option<i32>, Option<i32>)) {
        self.data_range = range;
    }

    pub fn create_sub_context(
        &'c mut self,
        index: i32,
        t: ContextType,
        var_count: i32,
        subctx_count: i32,
        libfun_count: i32,
    ) -> &mut Box<dyn Ctx<'a> + 'c>
    where
        'a: 'c,
        'b: 'c,
    {
        let mut subctx = Box::new(Context::new(None, t));
        subctx.init(var_count, subctx_count, libfun_count);
        unsafe {
            // Force the &Context to &mut Context to prevent the rust's borrow checker
            // When the sub context borrow the parent context, the parent context should not
            // use by the rust's borrow rules.

            // subctx.parent = Some(mem::transmute::<usize, &mut Context<'a, 'b, 'c>>(
            //     mem::transmute::<&Context<'a, 'b, 'c>, usize>(self),
            // ));
            // mem::transmute::<usize, &mut Context<'a, 'b, 'c>>(mem::transmute::<
            //     &Context<'a, 'b, 'c>,
            //     usize,
            // >(self))
            // .sub_contexts
            // .insert(name.clone(), subctx);
            let ptr: *mut Context<'a, 'b, 'c> = self;
            subctx.parent = Some(ptr.as_mut().unwrap());
            let context = ptr.as_mut().unwrap();
            context.sub_contexts[index as usize] = Some(subctx);
            // &mut context.sub_contexts[index as usize].unwrap()
        }
        self.get_sub_context(index).unwrap()
    }

    pub fn map_var<F>(&mut self, index: VarIndex, f: F)
    where
        F: Fn(Option<PineRef<'a>>) -> Option<PineRef<'a>>,
    {
        let context = downcast_ctx(self.get_subctx_mut(index));
        let val = mem::replace(&mut context.vars[index.varid as usize], None);
        if let Some(ret_val) = f(val) {
            context.vars[index.varid as usize] = Some(ret_val);
        }
    }

    pub fn commit(&mut self) {
        commit_series_for_operator(self);
        // Commit the Series for all of the sub context.
        for ctx in self.sub_contexts.iter_mut() {
            // If this context does not declare variables, so this context is not run,
            // we need not commit the series.
            if let Some(ctx) = ctx {
                if ctx.get_is_run() {
                    downcast_ctx(&mut **ctx).commit();
                }
            }
        }

        if !self.first_commit {
            self.first_commit = true;
        }
    }

    pub fn roll_back(&mut self) -> Result<(), PineRuntimeError> {
        let len = self.vars.len() as i32;
        for k in 0..len {
            let index = VarIndex::new(k, 0);
            let val = self.move_var(index).unwrap();
            let ret_val = match val.get_type() {
                (DataType::Float, SecondType::Series) => roll_back_series::<Float>(val),
                (DataType::Int, SecondType::Series) => roll_back_series::<Int>(val),
                (DataType::Color, SecondType::Series) => roll_back_series::<Color>(val),
                (DataType::Bool, SecondType::Series) => roll_back_series::<Bool>(val),
                _ => val,
            };
            self.update_var(index, ret_val);
        }
        let callables = mem::replace(&mut self.callables, vec![]);
        for callable in callables.iter() {
            if let Err(code) = callable.back(self) {
                return Err(PineRuntimeError::new_no_range(code));
            }
        }
        mem::replace(&mut self.callables, callables);
        Ok(())
    }

    pub fn run_callbacks(&mut self) -> Result<(), RuntimeErr> {
        let callables = mem::replace(&mut self.callables, vec![]);
        for callable in callables.iter() {
            callable.run(self)?;
        }
        mem::replace(&mut self.callables, callables);
        Ok(())
    }

    pub fn contains_sub_context(&self, index: i32) -> bool {
        self.sub_contexts[index as usize].is_some()
    }

    pub fn get_sub_context(&mut self, index: i32) -> Option<&mut Box<dyn Ctx<'a> + 'c>> {
        match &mut self.sub_contexts[index as usize] {
            Some(v) => Some(v),
            None => None,
        }
    }

    pub fn set_sub_context(&mut self, index: i32, sub_context: Box<dyn Ctx<'a> + 'c>) {
        self.sub_contexts[index as usize] = Some(sub_context);
    }

    pub fn update_sub_context(&mut self, index: i32, subctx: Box<dyn Ctx<'a> + 'c>) {
        self.sub_contexts[index as usize] = Some(subctx);
    }

    pub fn get_subctx_mut(&mut self, index: VarIndex) -> &mut dyn Ctx<'a> {
        let mut dest_ctx: &mut dyn Ctx<'a> = self;
        let mut rel_ctx = index.rel_ctx;
        debug_assert!(rel_ctx >= 0);
        while rel_ctx > 0 {
            dest_ctx = *downcast_ctx(dest_ctx).parent.as_mut().unwrap();
            rel_ctx -= 1;
        }
        dest_ctx
    }

    pub fn get_subctx(&self, index: VarIndex) -> &dyn Ctx<'a> {
        let mut dest_ctx: &dyn Ctx<'a> = self;
        let mut rel_ctx = index.rel_ctx;
        debug_assert!(rel_ctx >= 0);
        while rel_ctx > 0 {
            dest_ctx = *downcast_ctx_const(dest_ctx).parent.as_ref().unwrap();
            rel_ctx -= 1;
        }
        dest_ctx
    }
}

impl<'a, 'b, 'c> VarOperate<'a> for Context<'a, 'b, 'c> {
    fn create_var(&mut self, index: i32, val: PineRef<'a>) -> Option<PineRef<'a>> {
        mem::replace(&mut self.vars[index as usize], Some(val))
    }

    fn update_var(&mut self, index: VarIndex, val: PineRef<'a>) {
        let dest_ctx = downcast_ctx(self.get_subctx_mut(index));
        downcast_ctx(dest_ctx).vars[index.varid as usize] = Some(val);
    }

    // Move the value for the specific name from this context or the parent context.
    fn move_var(&mut self, index: VarIndex) -> Option<PineRef<'a>> {
        // Insert the temporary NA into the name and move the original value out.
        let dest_ctx = downcast_ctx(self.get_subctx_mut(index));
        mem::replace(&mut dest_ctx.vars[index.varid as usize], None)
    }

    fn get_var(&self, index: VarIndex) -> &Option<PineRef<'a>> {
        let dest_ctx = downcast_ctx_const(self.get_subctx(index));
        &dest_ctx.vars[index.varid as usize]
    }

    fn var_len(&self) -> i32 {
        self.vars.len() as i32
    }
}

impl<'a, 'b, 'c> Ctx<'a> for Context<'a, 'b, 'c> {
    fn contains_var_scope(&self, index: i32) -> bool {
        self.vars[index as usize].is_some()
    }

    fn contains_var(&self, index: VarIndex) -> bool {
        let dest_ctx = self.get_subctx(index);
        downcast_ctx_const(dest_ctx).vars[index.varid as usize].is_some()
    }

    fn create_callable(&mut self, call: RefData<Callable<'a>>) {
        if let Some(ref mut v) = self.parent {
            v.create_callable(call);
        } else if !self.first_commit {
            self.callables.push(call);
        }
    }

    fn move_fun_instance(&mut self, index: i32) -> Option<RefData<Callable<'a>>> {
        mem::replace(&mut self.fun_instances[index as usize], None)
    }

    fn create_fun_instance(&mut self, index: i32, val: RefData<Callable<'a>>) {
        self.fun_instances[index as usize] = Some(val);
    }

    fn set_is_run(&mut self, is_run: bool) {
        self.is_run = is_run;
    }

    fn get_is_run(&self) -> bool {
        self.is_run
    }

    fn clear_is_run(&mut self) {
        self.is_run = false;
        for subctx in self.sub_contexts.iter_mut() {
            if let Some(subctx) = subctx {
                subctx.clear_is_run();
            }
        }
    }

    fn get_type(&self) -> ContextType {
        self.context_type
    }

    fn get_callback(&self) -> Option<&'a dyn Callback> {
        self.callback
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct PineRuntimeError {
    pub code: RuntimeErr,
    pub range: StrRange,
}

impl PineRuntimeError {
    pub fn new(code: RuntimeErr, range: StrRange) -> PineRuntimeError {
        PineRuntimeError { code, range }
    }

    pub fn new_no_range(code: RuntimeErr) -> PineRuntimeError {
        PineRuntimeError {
            code,
            range: StrRange::from_start("", Position::new(0, 0)),
        }
    }
}

pub trait Runner<'a> {
    fn run(&'a self, context: &mut dyn Ctx<'a>) -> Result<PineRef<'a>, PineRuntimeError>;
}

// Evaluate  the expression with right-value.
pub trait RVRunner<'a> {
    fn rv_run(&'a self, context: &mut dyn Ctx<'a>) -> Result<PineRef<'a>, PineRuntimeError>;
}

pub trait StmtRunner<'a> {
    fn st_run(&'a self, context: &mut dyn Ctx<'a>) -> Result<(), PineRuntimeError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Int, PineFrom};

    #[test]
    fn context_test() {
        let mut context1 = Context::new(None, ContextType::Normal);
        context1.init_vars(vec![Some(PineRef::new(Some(1)))]);
        // context1.create_declare("hello");
        // assert!(context1.contains_declare("hello"));

        // context1.clear_declare();
        // assert!(!context1.contains_declare("hello"));

        context1.create_var(0, PineRef::new_box(Some(1)));
        assert_eq!(
            Int::implicity_from(context1.move_var(VarIndex::new(0, 0)).unwrap()),
            Ok(RefData::new_box(Some(1)))
        );

        context1.update_var(VarIndex::new(0, 0), PineRef::new_box(Some(10)));
        assert_eq!(
            Int::implicity_from(context1.move_var(VarIndex::new(0, 0)).unwrap()),
            Ok(RefData::new_box(Some(10)))
        );
        // assert!(context1.contains_var("hello"));

        context1.map_var(VarIndex::new(0, 0), |_| Some(PineRef::new_box(Some(100))));
        assert_eq!(
            Int::implicity_from(context1.move_var(VarIndex::new(0, 0)).unwrap()),
            Ok(RefData::new_box(Some(100)))
        );
    }

    #[test]
    fn callable_context_test() {
        // Parent context create callable
        let mut context1 = Context::new(None, ContextType::Normal);
        context1.create_callable(RefData::new_rc(Callable::new(None, None)));
        assert_eq!(context1.callables.len(), 1);

        {
            // Child context create callable
            let mut context2 = Context::new(Some(&mut context1), ContextType::Normal);
            context2.create_callable(RefData::new_rc(Callable::new(None, None)));
        }
        assert_eq!(context1.callables.len(), 2);

        context1.commit();

        // After commit, parent context and child context should not add callable by create callable
        context1.create_callable(RefData::new_rc(Callable::new(None, None)));
        {
            let mut context2 = Context::new(Some(&mut context1), ContextType::Normal);
            context2.create_callable(RefData::new_rc(Callable::new(None, None)));
        }
        assert_eq!(context1.callables.len(), 2);

        assert_eq!(context1.roll_back(), Ok(()));
        assert_eq!(context1.run_callbacks(), Ok(()));
        assert_eq!(context1.callables.len(), 2);
    }

    #[test]
    fn derive_context_test() {
        // hello is owned by context1, hello2 is owned by context2, hello3 is not owned by both context
        let mut context1 = Context::new(None, ContextType::Normal);
        context1.init_vars(vec![Some(PineRef::new_box(Some(1)))]);

        let mut context2 = Context::new(Some(&mut context1), ContextType::Normal);
        context2.init_vars(vec![Some(PineRef::new_box(Some(2)))]);

        assert_eq!(context2.contains_var(VarIndex::new(0, 1)), true);
        let mov_res1 = context2.move_var(VarIndex::new(0, 1)).unwrap();
        assert_eq!(mov_res1, PineRef::new(Some(1)));

        assert_eq!(context2.contains_var(VarIndex::new(0, 0)), true);
        let mov_res2 = context2.move_var(VarIndex::new(0, 0)).unwrap();
        assert_eq!(mov_res2, PineRef::new(Some(2)));

        // assert_eq!(context2.contains_var("hello3"), false);
        // assert_eq!(context2.move_var("hello3"), None);

        // context2.update_var(VarIndex::new(0, 1), mov_res1);
        // assert!(context2.vars.get(VarIndex).is_none());
        // {
        //     let parent = context2.parent.as_mut().unwrap();
        //     assert!(parent.move_var("hello").is_some());
        // }

        // context2.update_var("hello2", mov_res2);
        // assert!(context2.vars.get("hello2").is_some());

        // context2.update_var("hello3", PineRef::new(Some(10)));
        // assert!(context2.vars.get("hello3").is_some());

        // assert!(context2.contains_var("hello"));
        // assert!(context2.contains_var("hello2"));
    }
}
