//! Regression coverage for `generated/p7_2.md` gap #2 — cross-module
//! proto resolution in conformance lists, specifically for the
//! `builtin` module's marker protos (`Iterable` / `Iterator`).
//!
//! Before the fix, `proto Iterable {}` / `proto Iterator {}` in
//! `builtin.p7` were declared without `pub`. The qualified-name path
//! `struct[builtin.Iterator] X` already routed through
//! `resolve_qualified_type_name` and `import_type_from_module`'s
//! `TypeDefinition::Proto` arm, but the visibility check in
//! `bytecode/codegen/type_check.rs::resolve_qualified_type_name`
//! rejected the import as `TypeNotFound` because the proto was private.
//!
//! Making the two markers `pub` is sufficient: every other piece of
//! the import / conformance / dispatch pipeline is already TypeId-keyed
//! and treats imported protos identically to locally-declared ones.
//! These tests pin the behaviour.

use p7::InMemoryModuleProvider;
use p7::interpreter::context::Data;

#[test]
fn qualified_builtin_iterator_drives_for_in() {
    // Happy path: a user struct opts into `builtin.Iterable` /
    // `builtin.Iterator` via qualified-path conformance and drives a
    // proto-path `for-in`. Structural conformance is still checked at
    // the loop site; the explicit `[...]` opt-in is what unlocks
    // implicit coercion to `box<builtin.Iterable<Counter>>` at other sites
    // (covered by `box_coercion_via_qualified_builtin_proto_conformance`
    // below).
    let src = r#"
struct[builtin.Iterator<int>] Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

struct[builtin.Iterable<Counter>] Source(limit: int) {
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
fn unqualified_iterable_after_import_builtin_is_now_allowed_via_prelude() {
    // The `builtin` module is treated as a prelude: its public,
    // non-`@builtin()`, non-generic types are auto-imported into
    // root scope under their bare short name. `import builtin;` is
    // therefore redundant but harmless, and `struct[Iterable] ...`
    // works without any qualifier.
    //
    // NB: this is a special case for the `builtin` module. User
    // modules continue to follow the rule that `import M;` only
    // brings `M`'s name into scope (covered by
    // `import_user_module_does_not_auto_expose_members` below).
    let src = r#"
import builtin;

struct[Iterable<int>] Source(limit: int) {
    pub fn iter(ref self) -> box<int> { box(0) }
}

fn main() -> int { 0 }
"#;
    assert!(
        p7::compile(src.to_string()).is_ok(),
        "bare `Iterable` must resolve via the builtin prelude"
    );
}

#[test]
fn import_user_module_does_not_auto_expose_members() {
    // The auto-import policy applies ONLY to the `builtin` module.
    // For ordinary user modules, `import M;` brings just the module
    // name into scope; members must still be qualified `M.X`.
    let user_proto = r#"
pub proto MyProto {
    fn ping(ref self) -> int;
}
"#;
    let user = r#"
import my_mod;

struct[MyProto] X() {
    pub fn ping(ref self) -> int { 0 }
}

fn main() -> int { 0 }
"#;
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("my_mod".to_string(), user_proto.to_string());
    assert!(
        p7::compile_with_provider(user.to_string(), Box::new(provider)).is_err(),
        "bare `MyProto` (a member of an imported user module) must remain a \
         compile error; only `builtin` enjoys prelude semantics"
    );
}

#[test]
fn qualified_path_to_private_user_proto_errors() {
    // The visibility filter at `type_check.rs::resolve_qualified_type_name`
    // must continue to reject qualified access to a non-`pub` proto.
    // This pins that the fix to make `builtin.Iterable`/`Iterator`
    // public is opt-in per proto and not a blanket "ignore visibility"
    // change.
    let private_mod = r#"
proto PrivProto {
    fn ping(ref self) -> int;
}
"#;
    let user = r#"
import priv_mod;

struct[priv_mod.PrivProto] X() {
    pub fn ping(ref self) -> int { 0 }
}

fn main() -> int { 0 }
"#;
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("priv_mod".to_string(), private_mod.to_string());
    let result = p7::compile_with_provider(user.to_string(), Box::new(provider));
    assert!(
        result.is_err(),
        "non-`pub` proto must not be reachable through a qualified \
         conformance list"
    );
}

#[test]
fn qualified_and_unqualified_local_proto_resolve_to_same_typeid() {
    // The qualified path must reach the same TypeId as the local-scope
    // path when both reference the same proto. We approximate this
    // behaviourally: a struct conforming to `builtin.Iterable` and a
    // typed site referencing `box<builtin.Iterable<Counter>>` must share the
    // proto identity, otherwise the box coercion test above could
    // not pass. This test pins the round-trip explicitly with both
    // sides written using the qualified form.
    let src = r#"
struct[builtin.Iterator<int>] Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

struct[builtin.Iterable<Counter>] Source(limit: int) {
    pub fn iter(ref self) -> box<Counter> {
        box(Counter(0, self.limit))
    }
}

fn count(it: box<builtin.Iterable<Counter>>) -> int {
    return 0;
}

fn make() -> box<builtin.Iterable<Counter>> {
    return box(Source(3));
}

fn main() -> int {
    let it = make();
    count(it)
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(0)
    );
}

#[test]
fn box_coercion_via_qualified_builtin_proto_conformance() {
    // The whole point of explicit conformance over structural-only is
    // unlocking implicit coercion at typed sites (§18.6). A struct
    // listing `[builtin.Iterable]` must implicitly coerce to
    // `box<builtin.Iterable<Counter>>` at a parameter site.
    let src = r#"
struct[builtin.Iterator<int>] Counter(cur: int, end: int) {
    pub fn next(box self) -> ?int {
        if self.cur >= self.end { return null; }
        let v = self.cur;
        self.cur = v + 1;
        return v;
    }
}

struct[builtin.Iterable<Counter>] Source(limit: int) {
    pub fn iter(ref self) -> box<Counter> {
        box(Counter(0, self.limit))
    }
}

fn take(it: box<builtin.Iterable<Counter>>) -> int {
    return 42;
}

fn main() -> int {
    let s = Source(3);
    // Implicit coercion from `box<Source>` to `box<builtin.Iterable<Counter>>`:
    // legal because `Source` lists `[builtin.Iterable]` in its
    // conformance bracket.
    take(box(s))
}
"#;
    assert_eq!(
        p7::compile_and_run(src.to_string(), "main").unwrap(),
        Data::Int(42)
    );
}
