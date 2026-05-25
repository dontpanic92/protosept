//! Regression coverage for `generated/p7.md` §3.3.10 — match-on-int /
//! match-on-enum reliability gaps, and the or-pattern extension added at
//! the same time.
//!
//! Bugs fixed:
//!   B1: trailing comma after the last arm is now optional.
//!   B2: `true` / `false` literal patterns parse and run.
//!   B3: bare identifier patterns bind to the scrutinee (irrefutable).
//!   B4: non-exhaustive matches are rejected at compile time.
//!
//! Extension:
//!   E1: or-patterns `p1 | p2 | ... | pn` for literal / unit-variant
//!       alternatives (no bindings in v1).

use p7::compile_and_run;
use p7::interpreter::context::Data;

fn run_ok(src: &str) -> Data {
    compile_and_run(src.to_string(), "main").expect("compile + run")
}

fn run_err(src: &str) -> String {
    let err = compile_and_run(src.to_string(), "main").expect_err("expected error");
    format!("{}", err)
}

// ---------------------------------------------------------------------------
// B1 — trailing comma after the last arm is optional.
// ---------------------------------------------------------------------------
#[test]
fn b1_trailing_comma_optional_on_last_arm() {
    let src = r#"
        fn classify(x: int) -> int {
            match x {
                0 => 1,
                _ => 0
            }
        }
        fn main() -> int { classify(0) + classify(7) }
    "#;
    assert_eq!(run_ok(src), Data::Int(1));
}

// ---------------------------------------------------------------------------
// B2 — bool literal patterns `true` / `false`.
// ---------------------------------------------------------------------------
#[test]
fn b2_bool_literal_patterns_parse_and_run() {
    let src = r#"
        fn to_int(b: bool) -> int {
            match b {
                true => 1,
                false => 0,
            }
        }
        fn main() -> int { to_int(true) * 10 + to_int(false) }
    "#;
    assert_eq!(run_ok(src), Data::Int(10));
}

// ---------------------------------------------------------------------------
// B3 — bare identifier patterns bind to the scrutinee (irrefutable).
// ---------------------------------------------------------------------------
#[test]
fn b3_identifier_binding_pattern_binds_scrutinee() {
    let src = r#"
        fn f(x: int) -> int {
            match x {
                0 => 100,
                n => n * 2,
            }
        }
        fn main() -> int { f(0) + f(5) + f(11) }
    "#;
    // 100 + 10 + 22 = 132
    assert_eq!(run_ok(src), Data::Int(132));
}

#[test]
fn b3_wildcard_still_works_alongside_binding_branch() {
    let src = r#"
        fn f(x: int) -> int {
            match x {
                0 => 1,
                _ => 9,
            }
        }
        fn main() -> int { f(0) + f(42) }
    "#;
    assert_eq!(run_ok(src), Data::Int(10));
}

// ---------------------------------------------------------------------------
// B4 — exhaustiveness checks (spec §9.6.4).
// ---------------------------------------------------------------------------
#[test]
fn b4_non_exhaustive_int_match_rejected() {
    let src = r#"
        fn f(x: int) -> int {
            match x { 0 => 1, 1 => 2, }
        }
        fn main() -> int { f(0) }
    "#;
    let err = run_err(src);
    assert!(err.contains("Non-exhaustive match"), "{}", err);
    assert!(err.contains("int"), "{}", err);
}

#[test]
fn b4_non_exhaustive_bool_match_rejected() {
    let src = r#"
        fn f(b: bool) -> int {
            match b { true => 1, }
        }
        fn main() -> int { f(true) }
    "#;
    let err = run_err(src);
    assert!(err.contains("Non-exhaustive"), "{}", err);
    assert!(err.contains("false"), "{}", err);
}

#[test]
fn b4_non_exhaustive_enum_match_rejected() {
    let src = r#"
        pub enum Color(Red, Green, Blue);
        fn f(c: Color) -> int {
            match c { Color.Red => 1, Color.Green => 2, }
        }
        fn main() -> int { f(Color.Red) }
    "#;
    let err = run_err(src);
    assert!(err.contains("Non-exhaustive"), "{}", err);
    assert!(err.contains("Blue"), "{}", err);
}

#[test]
fn b4_wildcard_makes_match_exhaustive() {
    let src = r#"
        fn f(x: int) -> int {
            match x { 0 => 1, _ => 0, }
        }
        fn main() -> int { f(99) }
    "#;
    assert_eq!(run_ok(src), Data::Int(0));
}

#[test]
fn b4_binding_identifier_makes_match_exhaustive() {
    let src = r#"
        fn f(x: int) -> int {
            match x { 0 => 100, n => n, }
        }
        fn main() -> int { f(42) }
    "#;
    assert_eq!(run_ok(src), Data::Int(42));
}

#[test]
fn b4_enum_full_variant_listing_is_exhaustive() {
    let src = r#"
        pub enum Color(Red, Green, Blue);
        fn f(c: Color) -> int {
            match c { Color.Red => 1, Color.Green => 2, Color.Blue => 3, }
        }
        fn main() -> int { f(Color.Red) + f(Color.Green) + f(Color.Blue) }
    "#;
    assert_eq!(run_ok(src), Data::Int(6));
}

// ---------------------------------------------------------------------------
// E1 — or-patterns.
// ---------------------------------------------------------------------------
#[test]
fn e1_or_pattern_int_literals() {
    let src = r#"
        fn classify(n: int) -> int {
            match n {
                0 | 1 | 2 => 100,
                3 | 4 => 200,
                _ => 999,
            }
        }
        fn main() -> int {
            classify(0) + classify(2) + classify(4) + classify(7)
        }
    "#;
    // 100 + 100 + 200 + 999 = 1399
    assert_eq!(run_ok(src), Data::Int(1399));
}

#[test]
fn e1_or_pattern_bool_is_exhaustive_without_wildcard() {
    let src = r#"
        fn f(b: bool) -> int {
            match b { true | false => 7, }
        }
        fn main() -> int { f(true) + f(false) }
    "#;
    assert_eq!(run_ok(src), Data::Int(14));
}

#[test]
fn e1_or_pattern_enum_unit_variants() {
    let src = r#"
        pub enum Color(Red, Green, Blue);
        fn f(c: Color) -> int {
            match c {
                Color.Red | Color.Green => 1,
                Color.Blue => 2,
            }
        }
        fn main() -> int { f(Color.Red) + f(Color.Green) + f(Color.Blue) }
    "#;
    assert_eq!(run_ok(src), Data::Int(4));
}

#[test]
fn e1_or_pattern_enum_full_coverage_is_exhaustive() {
    let src = r#"
        pub enum E(A, B, C);
        fn f(e: E) -> int {
            match e { E.A | E.B | E.C => 7, }
        }
        fn main() -> int { f(E.A) + f(E.B) + f(E.C) }
    "#;
    assert_eq!(run_ok(src), Data::Int(21));
}

#[test]
fn e1_or_pattern_mixed_enum_unit_branches() {
    let src = r#"
        pub enum E(A, B: int, C);
        fn f(e: E) -> int {
            match e {
                E.A | E.C => 1,
                E.B(_) => 2,
            }
        }
        fn main() -> int { f(E.A) + f(E.C) + f(E.B(99)) }
    "#;
    assert_eq!(run_ok(src), Data::Int(4));
}

#[test]
fn e1_or_pattern_with_binding_rejected() {
    let src = r#"
        fn f(x: int) -> int {
            match x { 0 | n => 1, _ => 0, }
        }
        fn main() -> int { f(0) }
    "#;
    let err = run_err(src);
    assert!(
        err.contains("or-pattern alternatives must not introduce bindings"),
        "{}",
        err
    );
}

#[test]
fn e1_or_pattern_with_destructure_rejected() {
    let src = r#"
        pub enum E(A: int, B);
        fn f(e: E) -> int {
            match e { E.A(_) | E.B => 1, }
        }
        fn main() -> int { f(E.B) }
    "#;
    let err = run_err(src);
    assert!(
        err.contains("or-pattern alternatives must be literal or unit-variant patterns"),
        "{}",
        err
    );
}

#[test]
fn e1_or_pattern_non_exhaustive_partial_coverage_rejected() {
    let src = r#"
        pub enum E(A, B, C);
        fn f(e: E) -> int {
            match e { E.A | E.B => 1, }
        }
        fn main() -> int { f(E.A) }
    "#;
    let err = run_err(src);
    assert!(err.contains("Non-exhaustive"), "{}", err);
    assert!(err.contains("C"), "{}", err);
}

// ---------------------------------------------------------------------------
// Regression — keep the existing supported shapes working.
// ---------------------------------------------------------------------------
#[test]
fn regression_intent_style_dispatch_with_constants() {
    // Module-level `let` constants used in match arms. Per spec, bare
    // identifiers are bindings; constants must be referenced by literal
    // here. This test pins the recommended workaround.
    let src = r#"
        fn dispatch(code: int) -> int {
            match code {
                1 => 10,
                2 => 20,
                3 => 30,
                _ => 0,
            }
        }
        fn main() -> int { dispatch(1) + dispatch(2) + dispatch(3) + dispatch(99) }
    "#;
    assert_eq!(run_ok(src), Data::Int(60));
}

#[test]
fn regression_enum_payload_destructure_still_works() {
    let src = r#"
        pub enum Shape(Circle: int, Square: int);
        fn area(s: Shape) -> int {
            match s {
                Shape.Circle(r) => 3 * r * r,
                Shape.Square(side) => side * side,
            }
        }
        fn main() -> int { area(Shape.Circle(2)) + area(Shape.Square(3)) }
    "#;
    // 3*2*2 + 3*3 = 12 + 9 = 21
    assert_eq!(run_ok(src), Data::Int(21));
}

#[test]
fn regression_nested_match_still_works() {
    let src = r#"
        fn f(a: int, b: int) -> int {
            match a {
                0 => match b { 0 => 1, _ => 2, },
                _ => match b { 0 => 3, _ => 4, },
            }
        }
        fn main() -> int { f(0,0) + f(0,1) + f(1,0) + f(1,1) }
    "#;
    assert_eq!(run_ok(src), Data::Int(10));
}

#[test]
fn regression_string_scrutinee_works_with_wildcard() {
    let src = r#"
        fn f(s: string) -> int {
            match s { "hi" => 1, _ => 0, }
        }
        fn main() -> int { f("hi") + f("bye") }
    "#;
    assert_eq!(run_ok(src), Data::Int(1));
}
