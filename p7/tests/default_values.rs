//! Regression coverage for default values on function parameters and
//! record-struct fields. These let omitted trailing arguments fall back
//! to a const-evaluable default, and are the primary ergonomic lever for
//! the declarative UI layer (`Window(title="x")` with the rest defaulted).
//!
//! Both positional omission (`Text("hi")`) and named omission
//! (`Text(content="hi")`) are covered, as is referencing a module-level
//! constant from a default expression.

use p7::compile_and_run;
use p7::interpreter::context::Data;

fn run_ok(src: &str) -> Data {
    compile_and_run(src.to_string(), "main").expect("compile + run")
}

const PRELUDE: &str = r#"
    let WHITE: int = 7;
    struct Text(content: string, color: int = WHITE, size: int = 12);
    fn add(a: int, b: int = 10) -> int { a + b }
"#;

#[test]
fn struct_field_default_applied_named() {
    let src = format!("{PRELUDE}\nfn main() -> int {{ let t = Text(content = \"hi\"); t.color }}");
    assert_eq!(run_ok(&src), Data::Int(7));
}

#[test]
fn struct_field_default_applied_positional() {
    // Flutter-like: positional, omit all trailing defaults.
    let src = format!("{PRELUDE}\nfn main() -> int {{ let t = Text(\"hi\"); t.size }}");
    assert_eq!(run_ok(&src), Data::Int(12));
}

#[test]
fn struct_field_default_overridden() {
    let src = format!("{PRELUDE}\nfn main() -> int {{ let t = Text(\"hi\", 3); t.color }}");
    assert_eq!(run_ok(&src), Data::Int(3));
}

#[test]
fn function_param_default_omitted() {
    let src = format!("{PRELUDE}\nfn main() -> int {{ add(5) }}");
    assert_eq!(run_ok(&src), Data::Int(15));
}

#[test]
fn function_param_default_provided() {
    let src = format!("{PRELUDE}\nfn main() -> int {{ add(5, 1) }}");
    assert_eq!(run_ok(&src), Data::Int(6));
}
