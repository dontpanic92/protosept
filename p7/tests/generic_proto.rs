//! Regression coverage for generic proto declarations (see
//! `generated/p7_2.md` §1 "Protos are not generic"). Verifies parsing,
//! conformance checking with type-arg substitution, and dispatch through
//! `box<P<T>>` / `ref<P<T>>` receivers.

use p7::compile_and_run;
use p7::interpreter::context::Data;

#[test]
fn generic_proto_parses_and_no_op_program_runs() {
    // Bare declaration with a type parameter and bound should parse and
    // compile to an empty program.
    let src = r#"
proto Iter<T> { fn next(box self) -> ?T; }
fn main() -> int { 42 }
"#;
    assert_eq!(
        compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(42)
    );
}

#[test]
fn generic_proto_with_typed_conformance_dispatches() {
    // A user struct conforms to `Iter<int>`; calling `next()` through a
    // `box<Iter<int>>` should type-check and return `?int`.
    let src = r#"
proto Iter<T> { fn next(box self) -> ?T; }

pub struct[Iter<int>] Counter(pub cur: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= 3 {
            let n: ?int = null;
            n
        } else {
            let v: ?int = self.cur;
            self.cur = self.cur + 1;
            v
        }
    }
}

fn main() -> int {
    let mut c: box<Iter<int>> = box(Counter(0));
    let mut sum = 0;
    let mut done = 0;
    while done == 0 {
        let v = c.next();
        if v == null { done = 1; } else { sum = sum + v!; }
    }
    sum
}
"#;
    assert_eq!(
        compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(3)
    );
}

#[test]
fn generic_proto_wrong_element_type_fails_conformance() {
    // Counter returns ?int but declares conformance to Iter<string>:
    // compilation must fail.
    let src = r#"
proto Iter<T> { fn next(box self) -> ?T; }

pub struct[Iter<string>] Counter(pub cur: int) {
    pub fn next(box self) -> ?int {
        null
    }
}

fn main() -> int { 0 }
"#;
    let err = compile_and_run(src.to_string(), "main");
    assert!(err.is_err(), "expected conformance error, got: {:?}", err);
}

#[test]
fn proto_wrong_arity_in_conformance_bracket_fails() {
    let src = r#"
proto Iter<T> { fn next(box self) -> ?T; }

pub struct[Iter] Bad() {
    pub fn next(box self) -> ?int { null }
}

fn main() -> int { 0 }
"#;
    let err = compile_and_run(src.to_string(), "main");
    assert!(err.is_err(), "expected arity error, got: {:?}", err);
}

#[test]
fn non_generic_proto_still_works_unchanged() {
    // Sanity: the original marker-proto path remains intact.
    let src = r#"
proto Greeter { fn greet(ref self) -> int; }

pub struct[Greeter] Hello() {
    pub fn greet(ref self) -> int { 7 }
}

fn main() -> int {
    let h = Hello();
    h.greet()
}
"#;
    assert_eq!(
        compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(7)
    );
}
