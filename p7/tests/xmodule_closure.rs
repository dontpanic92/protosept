//! Regression for gaps.md #2: a p7 closure created in one module and invoked
//! p7-side from another module must run in its DEFINING module.
//!
//! Pre-fix, `Data::Closure` carried only `func_addr` + captures, and
//! `CallClosure` resolved that address against the CALLER's module, so invoking
//! a closure from a different module than where it was created failed with
//! `StackUnderflow` / parameter-index-out-of-bounds.

use p7::InMemoryModuleProvider;
use p7::interpreter::context::{Context, Data};

fn run_entry(user: &str, modules: &[(&str, &str)]) -> Data {
    let mut provider = InMemoryModuleProvider::new();
    for (name, src) in modules {
        provider.add_module(name.to_string(), src.to_string());
    }
    let module = p7::compile_with_provider(user.to_string(), Box::new(provider)).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("entry", Vec::new());
    ctx.resume().expect("run");
    ctx.stack[0].stack.pop().expect("result")
}

#[test]
fn closure_made_in_imported_module_invoked_in_screen() {
    // Closure constructed in `mklib`, captured `base`, invoked in the screen
    // module's frame.
    let mklib = r#"
pub fn make_adder(base: int) -> fn(int) -> int {
    (v: int) => base + v
}
"#;
    let user = r#"
import mklib;
pub fn entry() -> int {
    let f: fn(int) -> int = mklib.make_adder(100);
    f(8)
}
"#;
    assert_eq!(run_entry(user, &[("mklib", mklib)]), Data::Int(108));
}

#[test]
fn closure_made_in_screen_invoked_in_imported_module() {
    // Mirror direction: closure constructed in the screen module is handed to
    // an imported module's function, which invokes it p7-side.
    let runner = r#"
pub fn run(cb: fn() -> int) -> int {
    cb()
}
"#;
    let user = r#"
import runner;
pub fn entry() -> int {
    let x: int = 41;
    runner.run(() => x + 1)
}
"#;
    assert_eq!(run_entry(user, &[("runner", runner)]), Data::Int(42));
}

#[test]
fn closure_roundtrips_through_two_modules() {
    // Closure created in `mklib`, passed through `runner` (a third module),
    // and finally invoked there — two boundary crossings.
    let mklib = r#"
pub fn make_const(n: int) -> fn() -> int {
    () => n
}
"#;
    let runner = r#"
pub fn run(cb: fn() -> int) -> int {
    cb()
}
"#;
    let user = r#"
import mklib;
import runner;
pub fn entry() -> int {
    let c: fn() -> int = mklib.make_const(77);
    runner.run(c)
}
"#;
    assert_eq!(
        run_entry(user, &[("mklib", mklib), ("runner", runner)]),
        Data::Int(77)
    );
}
