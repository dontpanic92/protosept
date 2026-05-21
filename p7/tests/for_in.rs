//! Tests for proposal §3.1.4 — `for x in arr` / `for i, x in arr` loops.
//!
//! Semantics under test:
//! - The iterable is evaluated exactly once.
//! - `arr.len()` is snapshotted at loop entry; mid-loop pushes are NOT visited.
//! - When the element type is Copy-treated, `x` binds by value; otherwise it
//!   auto-binds as `ref<T>` so the body can call methods without copying.
//! - `for i, x in arr` additionally binds `i` to the 0-based iteration counter.
//! - `break`/`continue` work as in `while`/`loop`.

use p7::errors::Proto7Error;
use p7::interpreter::context::Data;

fn run(src: &str) -> Result<Data, Proto7Error> {
    p7::compile_and_run(src.to_string(), "main")
}

fn err(src: &str) -> Proto7Error {
    p7::compile(src.to_string()).expect_err("expected compile error")
}

#[test]
fn for_in_sums_int_array() {
    let src = r#"
fn main() -> int {
    let arr = [1, 2, 3, 4, 5];
    let mut sum = 0;
    for x in arr {
        sum = sum + x;
    }
    sum
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(15));
}

#[test]
fn for_in_indexed_form_binds_both_i_and_x() {
    let src = r#"
fn main() -> int {
    let arr = [10, 20, 30];
    let mut acc = 0;
    for i, x in arr {
        // i = 0,1,2 ; x = 10,20,30
        acc = acc + i * 100 + x;
    }
    acc
}
"#;
    // i*100 contributes 0 + 100 + 200 = 300
    // x contributes 10 + 20 + 30 = 60
    assert_eq!(run(src).expect("runs"), Data::Int(360));
}

#[test]
fn for_in_empty_array_skips_body() {
    let src = r#"
fn main() -> int {
    let arr: array<int> = [];
    let mut visited = 0;
    for x in arr {
        visited = visited + 1;
    }
    visited
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(0));
}

#[test]
fn for_in_length_snapshot_does_not_visit_mid_loop_push() {
    // Pushing into the array while iterating must NOT make new elements
    // visible to the same loop (length is snapshotted at entry). The array
    // is wrapped in `box<...>` so that the push is observable through the
    // original handle without moving it.
    let src = r#"
fn main() -> int {
    let arr: box<array<int>> = box([1, 2, 3]);
    let mut visited = 0;
    for x in arr {
        visited = visited + 1;
        arr.push(99);
    }
    visited
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(3));
}

#[test]
fn for_in_break_exits_loop_early() {
    let src = r#"
fn main() -> int {
    let arr = [1, 2, 3, 4, 5];
    let mut sum = 0;
    for x in arr {
        if x == 4 {
            break;
        }
        sum = sum + x;
    }
    sum
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(6));
}

#[test]
fn for_in_continue_skips_iteration() {
    let src = r#"
fn main() -> int {
    let arr = [1, 2, 3, 4, 5];
    let mut sum = 0;
    for x in arr {
        if x == 3 {
            continue;
        }
        sum = sum + x;
    }
    sum
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(12));
}

#[test]
fn for_in_nested_does_not_collide_hidden_locals() {
    let src = r#"
fn main() -> int {
    let outer = [1, 2, 3];
    let inner = [10, 20];
    let mut sum = 0;
    for a in outer {
        for b in inner {
            sum = sum + a + b;
        }
    }
    sum
}
"#;
    // outer 1,2,3 × inner 10,20:
    //   a=1: (1+10)+(1+20) = 32
    //   a=2: (2+10)+(2+20) = 34
    //   a=3: (3+10)+(3+20) = 36
    //   total = 102
    assert_eq!(run(src).expect("runs"), Data::Int(102));
}

#[test]
fn for_in_over_box_array_unwraps_one_layer() {
    let src = r#"
fn main() -> int {
    let arr: box<array<int>> = box([5, 7, 9]);
    let mut sum = 0;
    for x in arr {
        sum = sum + x;
    }
    sum
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(21));
}

#[test]
fn for_in_non_copy_struct_binds_as_ref() {
    // S is not Copy-treated (no `[Copy]` conformance), so `x` must auto-bind
    // as ref<S>; field access through the ref must work without an explicit
    // `ref()` wrapper — matching the `let t = ref(self.tabs[i]);` idiom
    // currently used in `yaobow_editor/scripts/main_editor.p7:53`.
    let src = r#"
struct S(v: int);

fn main() -> int {
    let arr = [S(1), S(2), S(3)];
    let mut sum = 0;
    for x in arr {
        sum = sum + x.v;
    }
    sum
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(6));
}

#[test]
fn for_in_iterable_evaluated_exactly_once() {
    // The iterable expression must not be re-evaluated each iteration.
    // We test by mutating a counter local inside an interpolated string
    // builder — actually, simpler: use a side-effecting expression via an
    // assignment that returns the array AND increments a counter via a
    // block expression. p7 supports block expressions as values.
    let src = r#"
fn main() -> int {
    let mut calls = 0;
    let mut arr = [1, 2, 3];
    for x in {
        calls = calls + 1;
        arr
    } {
        let _ = x;
    }
    calls
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(1));
}

#[test]
fn for_in_parse_error_duplicate_idents() {
    let src = r#"
fn main() -> int {
    let arr = [1];
    for a, a in arr {
    }
    0
}
"#;
    let _ = err(src);
}

#[test]
fn for_in_parse_error_missing_iterable() {
    let src = r#"
fn main() -> int {
    for x in {
    }
    0
}
"#;
    let _ = err(src);
}

#[test]
fn for_in_iterable_type_must_be_array() {
    let src = r#"
fn main() -> int {
    for x in 42 {
    }
    0
}
"#;
    let _ = err(src);
}
