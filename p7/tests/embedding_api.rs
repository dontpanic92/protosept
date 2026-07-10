use p7::embedding::{CallOutcome, Runtime};
use p7::interpreter::context::Data;
use p7::interpreter::native::{NativeSignature, NativeType};
use std::cell::Cell;
use std::rc::Rc;

unsafe extern "C" fn empty_extension(
    _api: *const p7::native_abi::P7HostApi,
) -> p7::native_abi::P7Status {
    p7::native_abi::P7Status::Ok
}

fn compile(source: &str) -> p7::bytecode::Module {
    p7::compile(source.to_string()).expect("compile")
}

#[test]
fn stateful_typed_native_function_is_callable() {
    let module = compile(
        r#"
@intrinsic(name="host.add")
fn host_add(a: int, b: int) -> int;

fn run() -> int {
    host_add(20, 22)
}
"#,
    );
    let calls = Rc::new(Cell::new(0));
    let callback_calls = calls.clone();
    let mut runtime = Runtime::new();
    runtime.register_native_function(
        "host.add",
        NativeSignature::new(
            vec![NativeType::Int, NativeType::Int],
            Some(NativeType::Int),
        ),
        move |_context, args| {
            callback_calls.set(callback_calls.get() + 1);
            let (Data::Int(lhs), Data::Int(rhs)) = (&args[0], &args[1]) else {
                unreachable!("signature validates native arguments");
            };
            Ok(Some(Data::Int(lhs + rhs)))
        },
    );
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("call") {
        CallOutcome::Returned(Some(Data::Int(value))) => assert_eq!(value, 42),
        other => panic!("unexpected call outcome: {other:?}"),
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn native_return_type_mismatch_becomes_trap() {
    let module = compile(
        r#"
@intrinsic(name="host.bad")
fn host_bad() -> int;

fn run() -> int {
    host_bad()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime.register_native_function(
        "host.bad",
        NativeSignature::new(Vec::new(), Some(NativeType::Int)),
        |_context, _args| Ok(Some(Data::string("wrong"))),
    );
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("call") {
        CallOutcome::Trapped(error) => {
            assert!(error.to_string().contains("expected return type Int"));
        }
        other => panic!("unexpected call outcome: {other:?}"),
    }
}

#[test]
fn missing_script_function_is_reported_without_panic() {
    let mut runtime = Runtime::new();
    runtime.load_module(compile("fn main() {}"));
    let error = runtime
        .call("missing", Vec::new())
        .expect_err("missing function should fail");
    assert!(matches!(error, p7::errors::RuntimeError::FunctionNotFound));
}

#[test]
fn rooted_callback_supports_reentrant_native_dispatch() {
    let module = compile(
        r#"
@intrinsic(name="host.invoke")
fn host_invoke(value: int) -> int;

fn make_callback() -> fn(int) -> int {
    let factor = 2;
    (value: int) => value * factor
}

fn run() -> int {
    host_invoke(21)
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime.load_module(module);
    let callback = match runtime
        .call("make_callback", Vec::new())
        .expect("make callback")
    {
        CallOutcome::Returned(Some(value @ Data::Closure { .. })) => {
            runtime.root_callback(value).expect("root callback")
        }
        other => panic!("unexpected callback result: {other:?}"),
    };
    runtime.context_mut().collect_garbage().expect("collect");

    runtime.register_native_function(
        "host.invoke",
        NativeSignature::new(vec![NativeType::Int], Some(NativeType::Int)),
        move |context, args| match callback.invoke(context, vec![args[0].clone()])? {
            CallOutcome::Returned(value) => Ok(value),
            CallOutcome::Threw(value) => Err(p7::errors::RuntimeError::Other(format!(
                "callback threw {value:?}"
            ))),
            CallOutcome::Trapped(error) => Err(error),
        },
    );

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Returned(Some(Data::Int(value))) => assert_eq!(value, 42),
        other => panic!("unexpected run result: {other:?}"),
    }
}

#[test]
fn roots_are_runtime_scoped() {
    let mut first = Runtime::new();
    first.load_module(compile("fn main() -> int { 1 }"));
    let root = first.root(Data::Int(1));

    let second = Runtime::new();
    let error = root
        .get(second.context())
        .expect_err("cross-runtime root should fail");
    assert!(error.to_string().contains("different runtime"));
}

#[test]
fn script_call_arity_is_checked_before_execution() {
    let mut runtime = Runtime::new();
    runtime.load_module(compile("fn add(a: int, b: int) -> int { a + b }"));
    let error = runtime
        .call("add", vec![Data::Int(1)])
        .expect_err("wrong arity should fail");
    assert!(error.to_string().contains("expects 2 argument(s), got 1"));
}

#[test]
fn runtime_remains_usable_after_trap() {
    let mut runtime = Runtime::new();
    runtime.load_module(compile(
        r#"
fn boom() -> int {
    let values = [1, 2, 3];
    values[10]
}

fn add(a: int, b: int) -> int {
    a + b
}
"#,
    ));

    assert!(matches!(
        runtime.call("boom", Vec::new()).expect("call boom"),
        CallOutcome::Trapped(_)
    ));
    match runtime
        .call("add", vec![Data::Int(20), Data::Int(22)])
        .expect("call after trap")
    {
        CallOutcome::Returned(Some(Data::Int(value))) => assert_eq!(value, 42),
        other => panic!("unexpected post-trap outcome: {other:?}"),
    }
    assert_eq!(runtime.context().stack.len(), 1);
}

#[test]
fn native_runtime_pointer_remains_stable_when_runtime_moves() {
    fn make_runtime() -> (Runtime, usize) {
        let mut runtime = Runtime::new();
        runtime
            .register_native_extension(empty_extension)
            .expect("register extension");
        let address = runtime.context() as *const _ as usize;
        (runtime, address)
    }

    let (runtime, address_before_move) = make_runtime();
    let address_after_move = runtime.context() as *const _ as usize;
    assert_eq!(address_after_move, address_before_move);
}
