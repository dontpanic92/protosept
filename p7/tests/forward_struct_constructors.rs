//! Regression tests for forward references between same-module struct/enum
//! constructors and method calls.
//!
//! Field-type forward references already work (see `recursive_types.rs`).
//! These tests cover the remaining gap: method bodies that construct
//! later-declared structs/enums or call methods on later-declared types.

use p7::compile_and_run;
use p7::interpreter::context::Data;

fn compile_source(source: &str) {
    p7::compile(source.to_string()).expect("source compiles");
}

#[test]
fn constructor_forward_reference_compiles() {
    // A::make_b constructs B, which is declared later in the same module.
    compile_source(
        r#"
pub struct A() {
    pub fn make_b() -> B { B(42) }
}
pub struct B(pub x: int);
"#,
    );
}

#[test]
fn cross_struct_method_call_forward_reference_compiles() {
    // A::call_b calls B::baz, B declared later.
    compile_source(
        r#"
pub struct A() {
    pub fn call_b() -> int { B().baz() }
}
pub struct B() {
    pub fn baz(ref self) -> int { 7 }
}
"#,
    );
}

#[test]
fn reverse_ordered_iterable_then_iterator_compiles_and_runs() {
    // Inline clone of builtin Range/RangeIter with the *Iterable* declared
    // before its iterator type. This is exactly the ordering the workaround
    // forbids in builtin.p7. After the fix, it must compile and run.
    let src = r#"
proto Iterable {
    fn iter(ref self) -> box<MyRangeIter>;
}

proto Iterator {
    fn next(box self) -> ?int;
}

pub struct[Iterable] MyRange(pub start: int, pub end: int) {
    pub fn iter(ref self) -> box<MyRangeIter> {
        box(MyRangeIter(self.start, self.end))
    }
}

pub struct[Iterator] MyRangeIter(pub cur: int, pub end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

fn main() -> int {
    let r = MyRange(0, 5);
    let mut sum = 0;
    for v in r {
        sum = sum + v;
    }
    sum
}
"#;
    assert_eq!(
        compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn enum_method_constructing_later_struct_compiles() {
    compile_source(
        r#"
pub enum E(Wrap: int) {
    pub fn make_holder(self) -> Holder { Holder(1) }
}
pub struct Holder(pub n: int);
"#,
    );
}
