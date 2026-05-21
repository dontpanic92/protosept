//! Regression tests for spec §12.6: `@builtin()` structs MUST NOT declare
//! concrete fields. The parser rejects field declarations with a clear,
//! attribute-aware diagnostic that names both the struct and the
//! `@builtin` attribute, instead of letting the violation surface later
//! as a misleading "expected 0 args, N provided" error at the call site.
//!
//! See `generated/p7_2.md` §10 for the motivation.

use p7::compile;
use p7::errors::{ParseError, Proto7Error};

fn parse_error_message(source: &str) -> String {
    match compile(source.to_string()) {
        Err(Proto7Error::ParseError(ParseError::Other { message, .. })) => message,
        Err(other) => panic!(
            "expected ParseError::Other, got: {:?}\nfor source:\n{}",
            other, source
        ),
        Ok(_) => panic!("expected compile error, but source compiled:\n{}", source),
    }
}

#[test]
fn rejects_builtin_struct_with_positional_fields() {
    let msg = parse_error_message(
        r#"
@builtin()
pub struct Range(pub start: int, pub end: int);
"#,
    );
    assert!(
        msg.contains("Range"),
        "diagnostic should name the struct, got: {}",
        msg
    );
    assert!(
        msg.contains("@builtin"),
        "diagnostic should mention @builtin attribute, got: {}",
        msg
    );
}

#[test]
fn rejects_builtin_struct_with_single_named_field() {
    let msg = parse_error_message(
        r#"
@builtin()
struct Handle(pub raw: int);
"#,
    );
    assert!(msg.contains("Handle") && msg.contains("@builtin"));
}

#[test]
fn rejects_builtin_struct_with_fields_and_method_body() {
    // Even when methods are also present, the field list is the violation.
    let msg = parse_error_message(
        r#"
@builtin()
pub struct Foo(pub x: int) {
    pub fn x(ref self) -> int { self.x }
}
"#,
    );
    assert!(msg.contains("Foo") && msg.contains("@builtin"));
}

#[test]
fn accepts_builtin_struct_with_empty_parens() {
    // Mirrors `@builtin() pub struct string()` in builtin.p7.
    compile(
        r#"
@builtin()
pub struct Thing();
"#
        .to_string(),
    )
    .expect("@builtin() struct with empty parens should parse");
}

#[test]
fn accepts_builtin_struct_with_semicolon_form() {
    // Mirrors spec §12.6 example: `@builtin() struct Handle;`.
    compile(
        r#"
@builtin()
struct Handle;
"#
        .to_string(),
    )
    .expect("@builtin() struct with semicolon form should parse");
}

#[test]
fn accepts_builtin_struct_with_methods_no_fields() {
    // Mirrors the existing `array<T>` / `string` shape: no fields, methods present.
    compile(
        r#"
@builtin()
pub struct Bar<T>() {
    pub fn noop(ref self) -> int { 0 }
}
"#
        .to_string(),
    )
    .expect("@builtin() struct with methods but no fields should parse");
}

#[test]
fn accepts_regular_struct_with_fields() {
    // Sanity check: the rule only applies to @builtin() structs.
    compile(
        r#"
pub struct Range(pub start: int, pub end: int);
"#
        .to_string(),
    )
    .expect("regular struct with fields should parse");
}
