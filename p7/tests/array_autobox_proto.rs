//! Regression coverage for implicit `T -> box<P>` auto-boxing of bare
//! struct/enum *values* at checking-context sites — the affordance that
//! lets declarative UI children be written as
//! `children = [Text(...), Button(...)]` (expected `array<box<Element>>`)
//! without an explicit `box(...)` per element.
//!
//! Also covers the soundness guard: a bare value whose type does NOT
//! declare the expected proto must be rejected at compile time (it used
//! to be silently accepted and miscompiled into a struct-ref element).

use p7::compile_and_run;
use p7::interpreter::context::Data;

fn run_ok(src: &str) -> Data {
    compile_and_run(src.to_string(), "main").expect("compile + run")
}

fn run_err(src: &str) -> String {
    let err = compile_and_run(src.to_string(), "main").expect_err("expected error");
    format!("{}", err)
}

const PRELUDE: &str = r#"
    proto Element { fn tag(self: ref<Element>) -> int; }
    struct[Element] Text(content: string) { pub fn tag(self: ref<Text>) -> int { 1 } }
    struct[Element] Button(label: string) { pub fn tag(self: ref<Button>) -> int { 2 } }
    fn first_tag(children: array<box<Element>>) -> int {
        for c in children { return c.tag(); }
        0
    }
    fn sum_tags(children: array<box<Element>>) -> int {
        let mut s = 0;
        for c in children { s = s + c.tag(); }
        s
    }
"#;

#[test]
fn autobox_bare_values_in_array_literal() {
    let src = format!(
        "{PRELUDE}\nfn main() -> int {{ let c: array<box<Element>> = [Text(\"a\"), Button(\"b\")]; sum_tags(c) }}"
    );
    assert_eq!(run_ok(&src), Data::Int(3));
}

#[test]
fn autobox_mixed_with_explicit_box() {
    let src = format!(
        "{PRELUDE}\nfn main() -> int {{ let c: array<box<Element>> = [Text(\"a\"), box(Button(\"b\"))]; sum_tags(c) }}"
    );
    assert_eq!(run_ok(&src), Data::Int(3));
}

#[test]
fn autobox_at_assignment_site() {
    let src = format!(
        "{PRELUDE}\nfn main() -> int {{ let p: box<Element> = Button(\"b\"); p.tag() }}"
    );
    assert_eq!(run_ok(&src), Data::Int(2));
}

#[test]
fn explicit_box_upcast_still_works() {
    let src = format!(
        "{PRELUDE}\nfn main() -> int {{ let c: array<box<Element>> = [box(Button(\"b\"))]; first_tag(c) }}"
    );
    assert_eq!(run_ok(&src), Data::Int(2));
}

#[test]
fn non_conforming_bare_value_is_rejected() {
    // `NotEl` does not declare `Element`; using a bare `NotEl` value
    // where `box<Element>` is expected must be a compile-time error, not
    // a silent miscompile.
    let src = format!(
        "{PRELUDE}\nstruct NotEl(x: int) {{ pub fn tag(self: ref<NotEl>) -> int {{ 9 }} }}\n\
         fn main() -> int {{ let c: array<box<Element>> = [NotEl(5)]; first_tag(c) }}"
    );
    let err = run_err(&src);
    assert!(
        err.contains("box<") && err.contains("NotEl"),
        "expected a type-mismatch mentioning box<Element> and NotEl, got: {err}"
    );
}
