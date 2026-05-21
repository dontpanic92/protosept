//! Regression tests for the LoopContext refactor that standardised every
//! loop on the *deferred* `continue` model.
//!
//! Each loop generator (`loop`, `while`, `for-in` array fast path, `for-in`
//! iterable proto path) must drain its pending `continue` patches via
//! `finalize_continue_patches_to(target)` before `finalize_loop_context`.
//! These tests pin the observable behaviour:
//!
//! 1. `while` with `continue` mid-body still terminates and reaches the
//!    condition check.
//! 2. `loop` with `continue` followed by `break` still terminates.
//! 3. `for-in` over an array with `continue` still advances the hidden index
//!    counter. This is the canary for the original §7 bug: with an eager
//!    continue target, `continue` would jump back to the condition check,
//!    skip the increment block, and loop forever.

use p7::compile_and_run;
use p7::errors::Proto7Error;
use p7::interpreter::context::Data;

fn run(src: &str) -> Result<Data, Proto7Error> {
    compile_and_run(src.to_string(), "main")
}

#[test]
fn while_continue_still_increments_and_terminates() {
    // Sum even numbers in [0, 10). `continue` after the increment must
    // route to the condition check, not skip the `i = i + 1` step.
    let src = r#"
fn main() -> int {
    let mut i = 0;
    let mut sum = 0;
    while i < 10 {
        let cur = i;
        i = i + 1;
        if cur % 2 == 1 {
            continue;
        }
        sum = sum + cur;
    }
    sum
}
"#;
    // 0 + 2 + 4 + 6 + 8 = 20
    assert_eq!(run(src).expect("runs"), Data::Int(20));
}

#[test]
fn loop_continue_then_break_terminates() {
    // Count up to 5 via `loop { ... continue }`, exiting via `break`.
    let src = r#"
fn main() -> int {
    let mut i = 0;
    let mut visits = 0;
    loop {
        i = i + 1;
        if i < 5 {
            continue;
        }
        visits = i;
        break;
    }
    visits
}
"#;
    assert_eq!(run(src).expect("runs"), Data::Int(5));
}

#[test]
fn for_in_array_continue_advances_index_counter() {
    // Canary for the original §7 bug. With the pre-refactor eager
    // continue target, `continue` jumped back to the condition check
    // without advancing the hidden index, looping forever.
    let src = r#"
fn main() -> int {
    let arr = [1, 2, 3, 4, 5, 6];
    let mut sum = 0;
    for x in arr {
        if x % 2 == 1 {
            continue;
        }
        sum = sum + x;
    }
    sum
}
"#;
    // 2 + 4 + 6 = 12
    assert_eq!(run(src).expect("runs"), Data::Int(12));
}

#[test]
fn for_in_indexed_continue_skips_only_targeted_iteration() {
    // Indexed for-in form: ensure both the iteration counter `i` and the
    // hidden loop index advance through `continue`.
    let src = r#"
fn main() -> int {
    let arr = [10, 20, 30, 40];
    let mut acc = 0;
    for i, x in arr {
        if i == 2 {
            continue;
        }
        acc = acc + x;
    }
    acc
}
"#;
    // Skip index 2 (value 30): 10 + 20 + 40 = 70.
    assert_eq!(run(src).expect("runs"), Data::Int(70));
}

#[test]
fn nested_loops_continue_targets_innermost() {
    // Verify the patch list is per-LoopContext: `continue` in the inner
    // for-in must patch only the inner increment block, leaving the outer
    // while's continue patches untouched.
    let src = r#"
fn main() -> int {
    let arr = [1, 2, 3];
    let mut outer = 0;
    let mut inner_sum = 0;
    while outer < 2 {
        for x in arr {
            if x == 2 {
                continue;
            }
            inner_sum = inner_sum + x;
        }
        outer = outer + 1;
    }
    inner_sum
}
"#;
    // Inner iter skips x=2: 1 + 3 = 4. Outer runs twice: 8.
    assert_eq!(run(src).expect("runs"), Data::Int(8));
}
