//! L2 probe: SAM (Single Abstract Method) coercion for closures.
//!
//! Verifies that a closure literal at a `box<P>` expected-type site,
//! where `P` is an object proto with exactly one abstract method, is
//! elaborated to an anonymous `struct[P]` + impl with the closure body
//! as the proto method, and dispatches through the proto's vtable.

use p7::interpreter::context::{Context, Data};

const NO_CAPTURE_SOURCE: &str = r#"
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn entry() -> int {
    run(() => 42)
}
"#;

const ONE_CAPTURE_SOURCE: &str = r#"
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn entry() -> int {
    let x: int = 100;
    run(() => x + 7)
}
"#;

const MULTIPLE_CAPTURES_SOURCE: &str = r#"
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn entry() -> int {
    let a: int = 3;
    let b: int = 7;
    let c: int = 11;
    run(() => a * 100 + b * 10 + c)
}
"#;

const PARAM_PASSING_SOURCE: &str = r#"
pub proto IIntAction {
    fn invoke(self: ref<IIntAction>, value: int) -> int;
}

pub fn run_with(action: box<IIntAction>, n: int) -> int {
    action.invoke(n)
}

pub fn entry() -> int {
    let base: int = 50;
    run_with((v: int) => base + v, 8)
}
"#;

const UNIT_RETURN_SOURCE: &str = r#"
pub proto IAction {
    fn invoke(self: ref<IAction>);
}

pub fn run(action: box<IAction>) {
    action.invoke()
}

pub fn entry() -> int {
    run(() => { });
    99
}
"#;

const MULTI_METHOD_PROTO_REJECTS_SAM_SOURCE: &str = r#"
pub proto IPair {
    fn first(self: ref<IPair>) -> int;
    fn second(self: ref<IPair>) -> int;
}

pub fn run(pair: box<IPair>) -> int {
    pair.first()
}

pub fn entry() -> int {
    run(() => 42)
}
"#;

const SIGNATURE_MISMATCH_REJECTS_SAM_SOURCE: &str = r#"
pub proto IIntAction {
    fn invoke(self: ref<IIntAction>, value: int) -> int;
}

pub fn run(a: box<IIntAction>) -> int {
    a.invoke(1)
}

pub fn entry() -> int {
    run(() => 0)
}
"#;

fn run_entry(src: &str) -> Data {
    let module = p7::compile(src.to_string()).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("entry", Vec::new());
    ctx.resume().expect("run");
    ctx.stack[0].stack.pop().expect("result")
}

#[test]
fn closure_with_no_captures_dispatches_via_sam() {
    assert_eq!(run_entry(NO_CAPTURE_SOURCE), Data::Int(42));
}

#[test]
fn closure_with_single_capture_dispatches_via_sam() {
    assert_eq!(run_entry(ONE_CAPTURE_SOURCE), Data::Int(107));
}

#[test]
fn closure_with_multiple_captures_dispatches_via_sam() {
    assert_eq!(run_entry(MULTIPLE_CAPTURES_SOURCE), Data::Int(381));
}

#[test]
fn closure_with_typed_parameters_dispatches_via_sam() {
    assert_eq!(run_entry(PARAM_PASSING_SOURCE), Data::Int(58));
}

#[test]
fn closure_with_unit_return_dispatches_via_sam() {
    assert_eq!(run_entry(UNIT_RETURN_SOURCE), Data::Int(99));
}

#[test]
fn closure_against_multi_method_proto_rejected() {
    // The proto has two abstract methods, so SAM is not applicable;
    // the closure falls through to ordinary closure codegen which then
    // fails to match `box<IPair>`.
    let err = p7::compile(MULTI_METHOD_PROTO_REJECTS_SAM_SOURCE.to_string())
        .expect_err("multi-method proto must not accept SAM closure");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("TypeMismatch") || msg.contains("type mismatch"),
        "expected a TypeMismatch-style error, got: {msg}"
    );
}

#[test]
fn closure_with_arity_mismatch_rejected() {
    // The proto's single method expects one non-self parameter; the
    // closure provides zero. Falls through to ordinary closure codegen
    // which produces a TypeMismatch against `box<IIntAction>`.
    let err = p7::compile(SIGNATURE_MISMATCH_REJECTS_SAM_SOURCE.to_string())
        .expect_err("arity mismatch must not accept SAM closure");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("TypeMismatch") || msg.contains("type mismatch"),
        "expected a TypeMismatch-style error, got: {msg}"
    );
}
