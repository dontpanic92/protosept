//! Tests for proposal §3.1.1 — bare expression-statements should pop
//! non-unit results so authors can write `ui.text("hi");` instead of
//! `let _ = ui.text("hi");`.
//!
//! See `generated/p7.md` for context.

use p7::errors::{Proto7Error, SemanticError};
use p7::interpreter::context::Data;

fn compile_and_run(src: &str) -> Result<Data, Proto7Error> {
    p7::compile_and_run(src.to_string(), "main")
}

fn compile_err(src: &str) -> Proto7Error {
    p7::compile(src.to_string()).expect_err("expected compile error")
}

#[test]
fn expression_statement_pops_non_unit_result() {
    // Three non-unit expression-statements in a row used to leak two
    // values onto the stack and either crash the VM or corrupt the
    // function return. With the new `ExpressionStatement` codegen the
    // first two are popped and only the trailing tail-value `3` is
    // returned.
    let src = r#"
fn main() -> int {
    1;
    2;
    3
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(3));
}

#[test]
fn expression_statement_unit_no_pop() {
    // A unit-returning expression statement must not emit Pop. We
    // verify behaviourally: running it must not underflow the stack
    // and the function must return its tail value cleanly.
    let src = r#"
fn side() {
}

fn main() -> int {
    side();
    side();
    42
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(42));
}

#[test]
fn expression_statement_tail_value_kept() {
    // A bare expression at block tail (no semicolon) is still the
    // block's value — Statement::Expression is unchanged.
    let src = r#"
fn main() -> int {
    1 + 2
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(3));
}

#[test]
fn expression_statement_keeps_stack_balanced_in_loops() {
    // A loop body with non-unit expression-statements that runs many
    // iterations must leave the operand stack balanced; otherwise the
    // loop would either overflow or the function would return a
    // wrong value.
    let src = r#"
fn make() -> int {
    7
}

fn main() -> int {
    let mut i: int = 0;
    while i < 100 {
        make();
        make();
        i = i + 1;
    }
    i
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(100));
}

#[test]
fn must_use_function_discard_errors() {
    // A `@must_use`-tagged function whose non-unit result is dropped
    // at statement position should raise DiscardedMustUseValue.
    let src = r#"
@must_use()
fn important() -> int {
    1
}

fn main() -> int {
    important();
    0
}
"#;
    let err = compile_err(src);
    match err {
        Proto7Error::SemanticError(SemanticError::DiscardedMustUseValue { .. }) => {}
        other => panic!("expected DiscardedMustUseValue, got {:?}", other),
    }
}

#[test]
fn must_use_function_let_binding_is_allowed() {
    // Explicitly binding the must-use result silences the diagnostic.
    let src = r#"
@must_use()
fn important() -> int {
    1
}

fn main() -> int {
    let _ = important();
    important()
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(1));
}

#[test]
fn let_underscore_remains_valid() {
    // The legacy `let _ = ...` idiom must still type-check and run
    // unchanged so the ~140 callsites that use it don't regress.
    let src = r#"
fn make() -> int {
    9
}

fn main() -> int {
    let _ = make();
    let _ = make();
    make()
}
"#;
    assert_eq!(compile_and_run(src).expect("runs"), Data::Int(9));
}
