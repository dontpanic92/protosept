//! Regression tests for Option B: dropping the readonly axis from p7.
//!
//! - `ref<T>` is now a (mutable) borrowed view; assigning through it works.
//! - `ref self` methods may mutate fields of `self`.
//! - The legacy `ref mut self` receiver form is rejected at parse time.

use p7::{compile, compile_and_run, interpreter::context::Data};

fn run(source: &str) -> Data {
    compile_and_run(source.to_string(), "entry").expect("compile_and_run")
}

#[test]
fn ref_let_binding_is_mutable() {
    let result = run(
        r#"
struct S(pub x: int);

pub fn entry() -> int {
    let mut s = S(0);
    let r: ref<S> = ref(s);
    r.x = 42;
    s.x
}
"#,
    );
    assert!(matches!(result, Data::Int(42)), "expected 42, got {:?}", result);
}

#[test]
fn ref_self_method_can_mutate_field() {
    let result = run(
        r#"
struct Counter(pub n: int) {
    pub fn bump(ref self) {
        self.n = self.n + 1;
    }
}

pub fn entry() -> int {
    let c = box(Counter(0));
    c.bump();
    c.bump();
    c.bump();
    c.n
}
"#,
    );
    assert!(matches!(result, Data::Int(3)), "expected 3, got {:?}", result);
}

#[test]
fn ref_mut_self_receiver_is_rejected() {
    let err = compile(
        r#"
struct S(pub n: int) {
    pub fn bump(ref mut self) { self.n = self.n + 1; }
}

pub fn entry() -> int { 0 }
"#
        .to_string(),
    )
    .expect_err("ref mut self should fail to parse");

    let msg = format!("{}", err);
    assert!(
        msg.contains("ref mut self") || msg.contains("`ref self`"),
        "expected migration hint, got: {msg}"
    );
}

#[test]
fn robox_type_is_rejected() {
    // robox<T> was never implemented; verify it remains an unknown type.
    let err = compile(
        r#"
struct S(pub n: int);

pub fn entry() -> int {
    let s: robox<S> = robox(S(0));
    0
}
"#
        .to_string(),
    )
    .expect_err("robox<T> should not type-check");
    let msg = format!("{}", err);
    assert!(
        msg.contains("robox") || msg.contains("Type") || msg.contains("type"),
        "expected type error, got: {msg}"
    );
}
