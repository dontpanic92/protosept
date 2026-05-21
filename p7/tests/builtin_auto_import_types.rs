//! Regression coverage for `generated/p7_2.md` gap #5 — auto-importing
//! eligible `pub` types from the `builtin` module into root scope as
//! bare-name aliases (in addition to the canonical qualified form
//! `builtin.<Name>`).
//!
//! Eligibility (see `bytecode/codegen/mod.rs::import_builtin_symbols`):
//!   * `pub`
//!   * NOT marked `@builtin()` (those types use type-form syntax such
//!     as `[1, 2, 3]` / `"…"` and have no positional constructor)
//!   * no type parameters
//!
//! In `builtin.p7` today this admits `Iterable`, `Iterator`, `Range`,
//! `RangeIter`, `RangeIncl`, `RangeInclIter`.

use p7::interpreter::context::Data;

#[test]
fn range_constructible_without_qualifier() {
    // Bare `Range(...)` constructor + bare `.iter()` / `.next()` method
    // dispatch through the same `source_module = "builtin"` path that the
    // qualified `builtin.Range(...)` form uses.
    let src = r#"
fn main() -> int {
    let r = Range(0, 5);
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
    // 0+1+2+3+4 = 10
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn range_incl_constructible_without_qualifier() {
    let src = r#"
fn main() -> int {
    let r = RangeIncl(1, 4);
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
    // 1+2+3+4 = 10
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn for_in_over_bare_range_works() {
    let src = r#"
fn main() -> int {
    let mut sum = 0;
    for x in Range(0, 6) {
        sum = sum + x;
    }
    sum
}
"#;
    // 0+1+2+3+4+5 = 15
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(15)
    );
}

#[test]
fn qualified_and_bare_range_share_typeid() {
    // Mix the qualified and bare forms in the same compilation unit.
    // If they referred to different TypeIds, passing a bare-`Range`
    // value into a function annotated with `builtin.Range` (or vice
    // versa) would fail type-checking. This test pins that they alias
    // to the same TypeId.
    let src = r#"
fn sum_qualified(r: builtin.Range) -> int {
    let mut s = 0;
    for x in r { s = s + x; }
    s
}

fn sum_bare(r: Range) -> int {
    let mut s = 0;
    for x in r { s = s + x; }
    s
}

fn main() -> int {
    let a = sum_qualified(Range(0, 4));
    let b = sum_bare(builtin.Range(0, 4));
    a * 10 + b
}
"#;
    // each loop sums to 6 → 6*10 + 6 = 66
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(66)
    );
}

#[test]
fn bare_iterable_proto_in_conformance_list_works() {
    // The marker protos are auto-imported too, so bare `[Iterable]` /
    // `[Iterator]` in the conformance bracket work without the
    // `builtin.` qualifier.
    let src = r#"
struct[Iterator] Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

struct[Iterable] Source(limit: int) {
    pub fn iter(ref self) -> box<Counter> {
        box(Counter(0, self.limit))
    }
}

fn main() -> int {
    let s = Source(5);
    let mut sum = 0;
    for x in s {
        sum = sum + x;
    }
    sum
}
"#;
    // 0+1+2+3+4 = 10
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(10)
    );
}

#[test]
fn user_struct_named_range_shadows_builtin_alias() {
    // The auto-import inserts the bare alias only when no symbol of
    // the same name is already in scope. After load_builtin returns,
    // the user's `struct Range(...)` is registered normally; the
    // user's name wins. The qualified `builtin.Range(...)` form
    // continues to work as the disambiguator.
    let src = r#"
struct Range(x: int);

fn main() -> int {
    let user = Range(7);
    let mut sum = 0;
    for v in builtin.Range(0, 4) {
        sum = sum + v;
    }
    // 7 from user + (0+1+2+3 = 6) from builtin = 13
    user.x + sum
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(13)
    );
}

#[test]
fn at_builtin_types_are_not_aliased_as_constructors() {
    // `array<T>`, `string`, `HashMap<K, V>` all carry `@builtin()`
    // and are reached via type-form syntax (literals, indexing).
    // Auto-import explicitly skips them so a positional
    // `array(...)` / `string(...)` / `HashMap(...)` call does NOT
    // become a struct constructor. Each line below should fail to
    // compile.
    let cases = [
        r#"fn main() -> int { let _ = array(); 0 }"#,
        r#"fn main() -> int { let _ = string(); 0 }"#,
        r#"fn main() -> int { let _ = HashMap(); 0 }"#,
    ];
    for src in cases {
        assert!(
            p7::compile(src.to_string()).is_err(),
            "expected compile error for `{}` — @builtin() types must \
             not become callable as bare constructors",
            src
        );
    }
}
