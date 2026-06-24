//! Regression tests for the two-strength borrow model:
//!
//! - `ref<T>` is a **read-only** borrowed view; writing through it is an error.
//! - `refmut<T>` is a **mutable** borrowed view; it may only be formed from a
//!   mutable place (a `let mut` binding, a `box<T>`, or a `refmut<T>`).
//! - `ref self` is a read-only receiver; `refmut self` is a mutable receiver.
//! - Interior mutation of a value struct needs no `box`, but is gated on the
//!   root binding being `let mut` (or a `box`/`refmut` handle).
//! - The legacy `ref mut self` receiver form is rejected at parse time.

use p7::{compile, compile_and_run, interpreter::context::Data};

fn run(source: &str) -> Data {
    compile_and_run(source.to_string(), "entry").expect("compile_and_run")
}

fn expect_compile_err(source: &str, needle: &str) {
    let err = compile(source.to_string()).expect_err("expected compile error");
    let msg = format!("{}", err);
    assert!(
        msg.contains(needle),
        "expected error containing {needle:?}, got: {msg}"
    );
}

#[test]
fn let_mut_struct_field_is_mutable_without_box() {
    let result = run(r#"
struct S(pub x: int);

pub fn entry() -> int {
    let mut s = S(0);
    s.x = 42;
    s.x
}
"#);
    assert!(matches!(result, Data::Int(42)), "got {:?}", result);
}

#[test]
fn let_struct_field_write_is_rejected() {
    expect_compile_err(
        r#"
struct S(pub x: int);
pub fn entry() -> int { let s = S(0); s.x = 42; s.x }
"#,
        "not a mutable place",
    );
}

#[test]
fn write_through_ref_is_rejected() {
    expect_compile_err(
        r#"
struct S(pub x: int);
pub fn entry() -> int {
    let mut s = S(0);
    let r: ref<S> = ref(s);
    r.x = 42;
    s.x
}
"#,
        "not a mutable place",
    );
}

#[test]
fn refmut_binding_is_mutable() {
    let result = run(r#"
struct S(pub x: int);

pub fn entry() -> int {
    let mut s = S(0);
    let r: refmut<S> = refmut(s);
    r.x = 42;
    s.x
}
"#);
    assert!(matches!(result, Data::Int(42)), "got {:?}", result);
}

#[test]
fn refmut_of_immutable_let_is_rejected() {
    expect_compile_err(
        r#"
struct S(pub x: int);
pub fn entry() -> int {
    let s = S(0);
    let r: refmut<S> = refmut(s);
    r.x = 42;
    s.x
}
"#,
        "not a mutable place",
    );
}

#[test]
fn refmut_self_method_mutates_on_let_mut() {
    let result = run(r#"
struct Counter(pub n: int) {
    pub fn bump(refmut self) {
        self.n = self.n + 1;
    }
}

pub fn entry() -> int {
    let mut c = Counter(0);
    c.bump();
    c.bump();
    c.bump();
    c.n
}
"#);
    assert!(matches!(result, Data::Int(3)), "got {:?}", result);
}

#[test]
fn refmut_self_method_mutates_on_box() {
    let result = run(r#"
struct Counter(pub n: int) {
    pub fn bump(refmut self) {
        self.n = self.n + 1;
    }
}

pub fn entry() -> int {
    let c = box(Counter(0));
    c.bump();
    c.bump();
    c.n
}
"#);
    assert!(matches!(result, Data::Int(2)), "got {:?}", result);
}

#[test]
fn refmut_self_call_on_immutable_let_is_rejected() {
    expect_compile_err(
        r#"
struct Counter(pub n: int) {
    pub fn bump(refmut self) { self.n = self.n + 1; }
}
pub fn entry() -> int { let c = Counter(0); c.bump(); c.n }
"#,
        "not a mutable place",
    );
}

#[test]
fn ref_self_method_cannot_mutate_field() {
    expect_compile_err(
        r#"
struct S(pub n: int) {
    pub fn bump(ref self) { self.n = self.n + 1; }
}
pub fn entry() -> int { 0 }
"#,
        "not a mutable place",
    );
}

#[test]
fn ref_self_read_method_works() {
    let result = run(r#"
struct S(pub n: int) {
    pub fn get(ref self) -> int { self.n }
}
pub fn entry() -> int { let s = S(7); s.get() }
"#);
    assert!(matches!(result, Data::Int(7)), "got {:?}", result);
}

#[test]
fn refmut_coerces_to_ref_parameter() {
    let result = run(r#"
struct S(pub n: int);
fn readit(r: ref<S>) -> int { r.n }
pub fn entry() -> int {
    let mut s = S(5);
    let rm: refmut<S> = refmut(s);
    readit(rm)
}
"#);
    assert!(matches!(result, Data::Int(5)), "got {:?}", result);
}

#[test]
fn ref_cannot_satisfy_refmut_parameter() {
    expect_compile_err(
        r#"
struct S(pub n: int);
fn writeit(r: refmut<S>) { r.n = 1; }
pub fn entry() -> int {
    let s = S(5);
    let r: ref<S> = ref(s);
    writeit(r);
    0
}
"#,
        "refmut",
    );
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
        msg.contains("refmut self") || msg.contains("ref mut self"),
        "expected migration hint, got: {msg}"
    );
}

#[test]
fn robox_type_is_rejected() {
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
