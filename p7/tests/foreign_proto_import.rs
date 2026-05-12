//! Tests for cross-module use of `@foreign` protos via `import`.
//!
//! Reproduces and locks down the historical bug that forced every consuming
//! script to inline a duplicate copy of every generated `@foreign proto`
//! declaration. The fix re-synthesises the hidden carrier struct + HostMethod
//! children in the importing module and cross-keys the runtime vtable across
//! modules so dispatch works through normal vtable lookup.

use p7::interpreter::context::{Context, Data};
use p7::{InMemoryModuleProvider, ModuleProvider};

const PROVIDER_SRC: &str = r#"
@foreign(dispatcher="counter.invoke", finalizer="counter.release", type_tag="counter.Counter")
pub proto Counter {
    fn answer(self: ref<Counter>) -> int;
}

@intrinsic(name="counter.make")
pub fn make_counter() -> box<Counter>;
"#;

fn host_make_counter(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    ctx.push_foreign("counter.Counter", 0)
}

fn host_counter_invoke(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let type_tag = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected type_tag string, got {:?}", other),
    };
    let method = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected method string, got {:?}", other),
    };
    let _vtable_slot = match frame.stack.pop() {
        Some(Data::Int(_)) => (),
        other => panic!("expected vtable_slot int, got {:?}", other),
    };
    let _return_ty = match frame.stack.pop() {
        Some(Data::Array(_)) => (),
        other => panic!("expected return_ty array, got {:?}", other),
    };
    assert_eq!(type_tag, "counter.Counter");

    let _handle = ctx.pop_foreign("counter.Counter")?;

    match method.as_str() {
        "answer" => {
            ctx.stack_frame_mut()?.stack.push(Data::Int(42));
        }
        other => panic!("unexpected method '{}'", other),
    }
    Ok(())
}

fn host_counter_release(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let _handle = match frame.stack.pop() {
        Some(Data::Int(_)) => (),
        other => panic!("expected handle int, got {:?}", other),
    };
    Ok(())
}

fn run_main(main_src: &str) -> Data {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("counters".to_string(), PROVIDER_SRC.to_string());

    let module = p7::compile_with_provider(main_src.to_string(), provider.clone_boxed())
        .expect("compile main");

    let mut ctx = Context::new();
    ctx.register_host_function("counter.make".to_string(), host_make_counter);
    ctx.register_host_function("counter.invoke".to_string(), host_counter_invoke);
    ctx.register_host_function("counter.release".to_string(), host_counter_release);
    ctx.register_foreign_type("counter.Counter", Some("counter.release"));

    ctx.load_module(module);
    ctx.push_function("run", Vec::new());
    ctx.resume().expect("run");

    ctx.stack[0].stack.pop().expect("result")
}

/// Main module imports the `@foreign` proto explicitly by name and calls a
/// method on a value returned by a host factory function defined in the same
/// provider module.
#[test]
fn import_foreign_proto_dispatches_via_qualified_type() {
    const MAIN: &str = r#"
import counters;

pub fn run() -> int {
    let c: box<counters.Counter> = counters.make_counter();
    c.answer()
}
"#;
    assert_eq!(run_main(MAIN), Data::Int(42));
}

/// Caller never names the `@foreign` proto directly: it imports the module
/// and stores the return value of a function that returns `box<F>`. The
/// carrier must still be re-synthesised in the caller so that the method call
/// dispatches correctly.
#[test]
fn import_foreign_proto_via_return_type_only() {
    const MAIN: &str = r#"
import counters;

pub fn run() -> int {
    let c = counters.make_counter();
    c.answer()
}
"#;
    assert_eq!(run_main(MAIN), Data::Int(42));
}
