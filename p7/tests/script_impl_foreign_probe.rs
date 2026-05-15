//! L1 probe: script struct implementing @foreign proto.
//!
//! Verifies the two-rule design for `box<T> -> box<P>` coercion (§18.5,
//! §18.6):
//!
//!   1. If `T` lists `P` in its conformance bracket (`struct[P] T(...)`),
//!      the coercion is **implicit** at expected-type sites (let-binding,
//!      function args).
//!   2. If `T` only *structurally* satisfies `P` (without listing it),
//!      the coercion requires an **explicit** `as box<P>` cast.
//!
//! Listed conformance also implies §18.7 dynamic dispatch routes through
//! the listed proto.

use p7::interpreter::context::{Context, Data};

const LISTED_SOURCE: &str = r#"
@foreign(dispatcher="action.invoke", type_tag="test.IAction")
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

struct[IAction] LaunchAction(
    game_id: int,
) {
    pub fn invoke(self: ref<Self>) -> int {
        self.game_id + 1000
    }
}

struct[IAction] __anon_42(
    captured_x: int,
    captured_y: int,
) {
    pub fn invoke(self: ref<Self>) -> int {
        self.captured_x * 10 + self.captured_y
    }
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn call_named() -> int {
    let a = box(LaunchAction(42));
    run(a)
}

pub fn call_anon() -> int {
    let a = box(__anon_42(3, 7));
    run(a)
}

pub fn call_named_let_annotated() -> int {
    let p: box<IAction> = box(LaunchAction(99));
    p.invoke()
}
"#;

// `Structural` satisfies `IAction` (same method signature) but does not
// list `IAction` in its conformance bracket. Per design rule 2, only
// explicit `as box<IAction>` should be accepted.
const STRUCTURAL_EXPLICIT_SOURCE: &str = r#"
@foreign(dispatcher="action.invoke", type_tag="test.IAction")
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

struct Structural(
    seed: int,
) {
    pub fn invoke(self: ref<Self>) -> int {
        self.seed * 2
    }
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn call_explicit() -> int {
    let s = box(Structural(21));
    run(s as box<IAction>)
}
"#;

// Same shape, but tries to use implicit coercion. Per rule 2 this must
// be rejected at compile time.
const STRUCTURAL_IMPLICIT_SOURCE: &str = r#"
@foreign(dispatcher="action.invoke", type_tag="test.IAction")
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

struct Structural(
    seed: int,
) {
    pub fn invoke(self: ref<Self>) -> int {
        self.seed * 2
    }
}

pub fn run(action: box<IAction>) -> int {
    action.invoke()
}

pub fn call_implicit() -> int {
    let s = box(Structural(21));
    run(s)
}
"#;

#[test]
fn listed_named_struct_dispatches() {
    let module = p7::compile(LISTED_SOURCE.to_string()).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("call_named", Vec::new());
    ctx.resume().expect("run");
    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(result, Data::Int(1042));
}

#[test]
fn listed_anon_struct_with_captures_dispatches() {
    let module = p7::compile(LISTED_SOURCE.to_string()).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("call_anon", Vec::new());
    ctx.resume().expect("run");
    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(result, Data::Int(37));
}

#[test]
fn listed_struct_implicit_coercion_at_let_binding() {
    let module = p7::compile(LISTED_SOURCE.to_string()).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("call_named_let_annotated", Vec::new());
    ctx.resume().expect("run");
    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(result, Data::Int(1099));
}

#[test]
fn structural_only_with_explicit_cast_compiles() {
    // Per design rule 2, an explicit `as box<P>` cast is accepted at
    // compile time even when `T` only structurally satisfies `P`
    // without listing it (`generate_wrapper_to_proto_cast` falls back
    // to a structural conformance check). Runtime dispatch through
    // such a box currently fails because the vtable
    // (`Context::build_vtable`) is constructed only from listed
    // `conforming_to`; vtable population for structural-only
    // conformance is a separate runtime gap that is intentionally
    // out of scope for this language-level coercion fix.
    p7::compile(STRUCTURAL_EXPLICIT_SOURCE.to_string()).expect("compile");
}

#[test]
fn structural_only_without_listing_rejects_implicit_coercion() {
    let err = p7::compile(STRUCTURAL_IMPLICIT_SOURCE.to_string())
        .expect_err("structural-only conformance must require explicit `as` cast");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("TypeMismatch") || msg.contains("type mismatch"),
        "expected a TypeMismatch-style error, got: {msg}"
    );
}
