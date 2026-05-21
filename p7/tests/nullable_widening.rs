//! Regression tests for spec §3.5 — implicit `T -> ?T` widening at
//! checking/expected-type sites, including `null` synthesis against an
//! expected `?T`.
//!
//! Two specific gaps motivated these tests:
//!
//! 1. `generate_array_literal` used to ignore the array's expected
//!    element type when the literal was non-empty, so `[null]` and
//!    `[some_T]` could not widen against an `array<?T>` annotation.
//!
//! 2. `type_to_parsed_type` had no arm for `Type::Nullable` /
//!    `Type::Proto`, so the array-method substitution path emitted a
//!    synthetic `ParsedType::Identifier("?box<proto(N)>")` and failed
//!    with `TypeNotFound` the moment any method (`.len()`, `.push(..)`,
//!    etc.) was called on an `array<?box<P>>`.

use p7::InMemoryModuleProvider;

fn compile(source: &str) -> Result<(), p7::errors::Proto7Error> {
    p7::compile(source.to_string()).map(|_| ())
}

fn compile_with_radiance(user: &str) -> Result<(), p7::errors::Proto7Error> {
    const RADIANCE: &str = r#"
pub proto IFoo {
    fn ping(self: ref<IFoo>) -> int;
}
"#;
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("radiance".to_string(), RADIANCE.to_string());
    p7::compile_with_provider(user.to_string(), Box::new(provider)).map(|_| ())
}

#[test]
fn let_annotation_widens_t_to_nullable() {
    compile(
        r#"
pub fn entry() -> int {
    let r: ?int = 42;
    0
}
"#,
    )
    .expect("`let r: ?int = 42;` must widen via spec §3.5");
}

#[test]
fn let_annotation_accepts_null_against_nullable() {
    compile(
        r#"
pub fn entry() -> int {
    let r: ?int = null;
    0
}
"#,
    )
    .expect("`let r: ?int = null;` must typecheck (spec §4.5)");
}

#[test]
fn array_literal_widens_null_element_against_annotation() {
    compile(
        r#"
pub fn entry() -> int {
    let xs: array<?int> = [null];
    0
}
"#,
    )
    .expect("`[null]` must check against `array<?int>` via element-level widening");
}

#[test]
fn array_literal_widens_mixed_elements_against_annotation() {
    compile(
        r#"
pub fn entry() -> int {
    let xs: array<?int> = [1, null, 2];
    0
}
"#,
    )
    .expect("Mixed `T` / `null` elements must widen against `array<?T>`");
}

#[test]
fn array_of_nullable_box_proto_method_call_resolves() {
    // Regression for the `TypeNotFound { name: "?box<proto(N)>" }`
    // error that fires the moment a method is invoked on an
    // `array<?box<P>>`. The array-method substitution path uses
    // `type_to_parsed_type` to materialize the element type as a
    // ParsedType; the missing `Nullable` / `Proto` arms used to
    // produce a synthetic identifier that no scope ever defines.
    compile_with_radiance(
        r#"
import radiance;

pub fn entry(xs: array<?box<radiance.IFoo>>) -> int {
    xs.len()
}
"#,
    )
    .expect("array-method dispatch must work for `array<?box<P>>` receivers");
}

#[test]
fn array_literal_of_nullable_box_proto_widens_null() {
    compile_with_radiance(
        r#"
import radiance;

pub fn entry() -> int {
    let xs: array<?box<radiance.IFoo>> = [null];
    xs.len()
}
"#,
    )
    .expect("`[null]` must widen into `array<?box<P>>` and survive `.len()` dispatch");
}

#[test]
fn return_type_widens_t_to_nullable() {
    compile(
        r#"
pub fn pick(x: int) -> ?int {
    x
}
"#,
    )
    .expect("returning a `T` from a `?T`-typed function must widen");
}

#[test]
fn null_without_expected_type_is_rejected() {
    let err = compile(
        r#"
pub fn entry() -> int {
    let z = null;
    0
}
"#,
    )
    .err()
    .expect("`let z = null;` (no annotation) must still be rejected — spec §4.5");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("null") || msg.to_lowercase().contains("nullable"),
        "expected a null-related error, got: {}",
        msg
    );
}

#[test]
fn widening_does_not_bypass_inner_type_mismatch() {
    let err = compile(
        r#"
pub fn entry() -> int {
    let r: ?int = "hello";
    0
}
"#,
    )
    .err()
    .expect("widening must only fire when the inner type matches");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("TypeMismatch") || msg.to_lowercase().contains("type"),
        "expected a TypeMismatch-shaped error, got: {}",
        msg
    );
}
