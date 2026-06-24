//! Regression tests: value (non-boxed) `array<T>` mutation gated on `let mut`,
//! mirroring Swift's `var` arrays. No `box<array<T>>` is required for a local
//! mutable array; mutation through an immutable `let` is rejected.

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
fn let_mut_value_array_element_assign() {
    let result = run(r#"
pub fn entry() -> int {
    let mut xs = [1, 2, 3];
    xs[0] = 9;
    xs[2] = xs[0];
    xs[0] + xs[1] + xs[2]
}
"#);
    assert!(matches!(result, Data::Int(20)), "got {:?}", result);
}

#[test]
fn let_value_array_element_assign_rejected() {
    expect_compile_err(
        r#"
pub fn entry() -> int { let xs = [1,2,3]; xs[0] = 9; xs[0] }
"#,
        "not a mutable place",
    );
}

#[test]
fn let_mut_value_array_push_and_len() {
    let result = run(r#"
pub fn entry() -> int {
    let mut xs = [1, 2, 3];
    xs.push(4);
    xs.push(5);
    xs.len()
}
"#);
    assert!(matches!(result, Data::Int(5)), "got {:?}", result);
}

#[test]
fn let_mut_value_array_pop() {
    let result = run(r#"
pub fn entry() -> int {
    let mut xs = [10, 20, 30];
    let p = xs.pop();
    p! + xs.len()
}
"#);
    assert!(matches!(result, Data::Int(32)), "got {:?}", result);
}

#[test]
fn let_mut_value_array_insert_remove() {
    let result = run(r#"
pub fn entry() -> int {
    let mut xs = [1, 2, 3];
    xs.insert(1, 99);
    let r = xs.remove(0);
    r! + xs[0] + xs[1] + xs[2]
}
"#);
    // r=1, xs=[99,2,3] -> 1+99+2+3 = 105
    assert!(matches!(result, Data::Int(105)), "got {:?}", result);
}

#[test]
fn let_value_array_push_rejected() {
    expect_compile_err(
        r#"
pub fn entry() -> int { let xs = [1,2,3]; xs.push(4); xs.len() }
"#,
        "not a mutable place",
    );
}

#[test]
fn boxed_array_mutation_still_works() {
    let result = run(r#"
pub fn entry() -> int {
    let xs = box([1, 2, 3]);
    xs.push(4);
    xs[0] = 9;
    xs[0] + xs.len()
}
"#);
    assert!(matches!(result, Data::Int(13)), "got {:?}", result);
}

#[test]
fn value_array_read_methods_work() {
    let result = run(r#"
pub fn entry() -> int {
    let xs = [5, 6, 7];
    xs.len() + xs[1]
}
"#);
    assert!(matches!(result, Data::Int(9)), "got {:?}", result);
}
