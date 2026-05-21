use p7::compile_and_run;
use p7::interpreter::context::Data;

#[test]
fn range_manual_iteration_works() {
    // Range/RangeIter are added to builtin.p7. Iterating manually
    // (without `for-in` dispatching through `Iterable`) should already
    // work because the protos are non-generic markers.
    let src = r#"
fn main() -> int {
    let r = builtin.Range(0, 5);
    let it = r.iter();
    let mut sum = 0;
    let mut done = 0;
    while done == 0 {
        let v = it.next();
        if v == null { done = 1; } else { sum = sum + v!; }
    }
    sum
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn range_is_immutable_across_iterations() {
    let src = r#"
fn main() -> int {
    let r = builtin.Range(0, 3);
    let mut total = 0;
    let it1 = r.iter();
    let mut done = 0;
    while done == 0 {
        let v = it1.next();
        if v == null { done = 1; } else { total = total + 1; }
    }
    // Second pass: r.start must still be 0 (Range itself wasn't mutated).
    let it2 = r.iter();
    done = 0;
    while done == 0 {
        let v = it2.next();
        if v == null { done = 1; } else { total = total + 1; }
    }
    total * 10 + r.start
}
"#;
    // 3 + 3 = 6 iterations; r.start still 0 → 6*10 + 0 = 60
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(60)
    );
}

#[test]
fn range_incl_includes_endpoint() {
    let src = r#"
fn main() -> int {
    let r = builtin.RangeIncl(1, 3);
    let it = r.iter();
    let mut sum = 0;
    let mut done = 0;
    while done == 0 {
        let v = it.next();
        if v == null { done = 1; } else { sum = sum + v!; }
    }
    sum
}
"#;
    // 1 + 2 + 3 = 6
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(6)
    );
}

#[test]
fn for_in_dispatches_through_range() {
    let src = r#"
fn main() -> int {
    let mut sum = 0;
    for i in builtin.Range(0, 5) {
        sum = sum + i;
    }
    sum
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn for_in_indexed_form_over_range() {
    let src = r#"
fn main() -> int {
    let mut acc = 0;
    for idx, x in builtin.Range(10, 13) {
        // idx ∈ {0,1,2}; x ∈ {10,11,12}
        acc = acc + idx * 100 + x;
    }
    acc
}
"#;
    // idx*100 = 0+100+200 = 300; x sum = 33; total = 333
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(333)
    );
}

#[test]
fn for_in_range_break_continue() {
    let src = r#"
fn main() -> int {
    let mut sum = 0;
    for i in builtin.Range(0, 10) {
        if i == 5 { break; }
        if i == 2 { continue; }
        sum = sum + i;
    }
    sum
}
"#;
    // 0+1+3+4 = 8
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(8)
    );
}

#[test]
fn for_in_empty_range_runs_no_iterations() {
    let src = r#"
fn main() -> int {
    let mut visited = 0;
    for i in builtin.Range(5, 5) {
        visited = visited + 1;
    }
    for i in builtin.Range(7, 3) {
        visited = visited + 1;
    }
    visited
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(0)
    );
}

#[test]
fn for_in_range_incl_includes_endpoint() {
    let src = r#"
fn main() -> int {
    let mut sum = 0;
    for i in builtin.RangeIncl(1, 4) {
        sum = sum + i;
    }
    sum
}
"#;
    // 1+2+3+4 = 10
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn for_in_range_is_reusable() {
    // Iterating the same `Range` value twice yields the same sequence
    // because `iter()` returns a fresh `RangeIter`. The Range itself
    // is not mutated.
    let src = r#"
fn main() -> int {
    let r = builtin.Range(0, 4);
    let mut a = 0;
    for i in r {
        a = a + i;
    }
    let mut b = 0;
    for j in r {
        b = b + j;
    }
    // Range value untouched: r.start still 0 after both loops.
    a * 100 + b * 10 + r.start
}
"#;
    // a = b = 0+1+2+3 = 6; r.start = 0; → 6*100 + 6*10 + 0 = 660
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(660)
    );
}

#[test]
fn for_in_user_defined_iterable_structural_conformance() {
    // No `[Iterable]` / `[Iterator]` conformance declared — structural
    // conformance is sufficient (the compiler resolves `.iter()` and
    // `.next()` by name + signature on the concrete type).
    let src = r#"
struct Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

struct Source(limit: int) {
    pub fn iter(ref self) -> box<Counter> {
        box(Counter(0, self.limit))
    }
}

fn main() -> int {
    let s = Source(4);
    let mut sum = 0;
    for x in s {
        sum = sum + x;
    }
    sum
}
"#;
    // 0+1+2+3 = 6
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(6)
    );
}

#[test]
fn for_in_user_defined_iterable_explicit_conformance_with_qualified_proto() {
    // p7 today does not resolve `struct[Iterator]` (or `[builtin.Iterator]`)
    // in the conformance list against protos from the builtin module —
    // conformance-name resolution happens before the cross-module type
    // table is consulted. Until that gap is closed, authors who want
    // explicit-conformance opt-in declare the proto locally and dispatch
    // continues to work via structural conformance (covered by the test
    // above). This test documents the current limitation behaviourally
    // by asserting the parse-time error remains observable.
    let src = r#"
import builtin;

struct[Iterator] Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int { return null; }
}

fn main() -> int { 0 }
"#;
    let result = p7::compile(src.to_string());
    assert!(result.is_err(), "explicit conformance to builtin.Iterator should fail until cross-module proto resolution lands");
}

#[test]
fn for_in_non_iterable_value_is_a_compile_error() {
    // An integer (or any value lacking an `iter` method) must not be
    // accepted as a for-in iterable. The compiler should reject this
    // at parse / type-check time, NOT at runtime.
    let src = r#"
fn main() -> int {
    for x in 42 {
    }
    0
}
"#;
    let result = p7::compile(src.to_string());
    assert!(result.is_err(), "iterating over an int must be a compile error");
}

#[test]
fn for_in_struct_without_iter_method_is_a_compile_error() {
    let src = r#"
struct NoIter(v: int);

fn main() -> int {
    let n = NoIter(7);
    for x in n {
    }
    0
}
"#;
    let result = p7::compile(src.to_string());
    assert!(result.is_err(), "struct without an iter() method must be a compile error");
}
