//! Regression for gaps.md #6: calling a closure / IAction stored in a struct
//! field directly — `self.on_click()` or `(self.on_click)()` — must work
//! without first binding the field to a local.
//!
//! Pre-fix, `recv.field(...)` only resolved *methods*, so a same-named callable
//! field produced `FunctionNotFound`; the field had to be bound to a local
//! first (`let cb = self.on_click; cb();`).

use p7::interpreter::context::Data;
use p7::{compile_and_run, interpreter::context::Context};

fn run(src: &str) -> Data {
    compile_and_run(src.to_string(), "entry").expect("compile_and_run")
}

#[test]
fn closure_field_called_directly() {
    let result = run(r#"
struct Widget(pub on_click: fn() -> int);

pub fn invoke(w: ref<Widget>) -> int {
    w.on_click()
}

pub fn entry() -> int {
    let w = Widget(() => 42);
    invoke(ref(w))
}
"#);
    assert_eq!(result, Data::Int(42));
}

#[test]
fn closure_field_called_via_self_no_local_binding() {
    let result = run(r#"
struct Button(pub on_click: fn(int) -> int) {
    pub fn fire(self: ref<Self>, n: int) -> int {
        self.on_click(n)
    }
}

pub fn entry() -> int {
    let base: int = 100;
    let b = Button((v: int) => base + v);
    b.fire(8)
}
"#);
    assert_eq!(result, Data::Int(108));
}

#[test]
fn closure_field_called_via_parenthesized_form() {
    let result = run(r#"
struct Button(pub on_click: fn() -> int) {
    pub fn fire(self: ref<Self>) -> int {
        (self.on_click)()
    }
}

pub fn entry() -> int {
    let b = Button(() => 7);
    b.fire()
}
"#);
    assert_eq!(result, Data::Int(7));
}

#[test]
fn iaction_field_called_directly() {
    // A single-method `box<P>` field invoked as `self.on_click()` lowers to the
    // proto's sole method (here `invoke`).
    let result = run(r#"
pub proto IAction {
    fn invoke(self: ref<IAction>) -> int;
}

struct Holder(pub on_click: box<IAction>) {
    pub fn fire(self: ref<Self>) -> int {
        self.on_click()
    }
}

pub fn entry() -> int {
    let h = Holder(() => 55);
    h.fire()
}
"#);
    assert_eq!(result, Data::Int(55));
}

#[test]
fn nonexistent_field_still_errors() {
    // Calling a method/field that doesn't exist must still be a compile error,
    // not silently accepted.
    let err = p7::compile(
        r#"
struct Widget(pub value: int);

pub fn entry() -> int {
    let w = Widget(1);
    w.missing()
}
"#
        .to_string(),
    )
    .expect_err("calling a nonexistent member must fail to compile");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("FunctionNotFound") || msg.contains("missing"),
        "expected a not-found error, got: {msg}"
    );
}

#[test]
fn non_callable_field_call_still_errors() {
    // An int-typed field is not callable; `w.value()` must not compile.
    let err = p7::compile(
        r#"
struct Widget(pub value: int);

pub fn entry() -> int {
    let w = Widget(1);
    w.value()
}
"#
        .to_string(),
    )
    .expect_err("calling a non-callable field must fail to compile");
    let msg = format!("{err:?}");
    assert!(!msg.is_empty(), "expected an error");
    let _ = Context::new();
}
