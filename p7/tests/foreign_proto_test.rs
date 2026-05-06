//! End-to-end tests for `@foreign` proto support.

use p7::interpreter::context::{Context, Data};

static mut COUNTER_VALUE: i64 = 0;
static mut FINALIZER_CALLS: i64 = 0;

fn reset_globals() {
    unsafe {
        COUNTER_VALUE = 0;
        FINALIZER_CALLS = 0;
    }
}

fn host_make_counter(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    ctx.push_foreign("counter.Counter", 0)
}

fn host_counter_invoke(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    // New protocol (top → bottom):
    //   type_tag, method_name, vtable_slot, return_ty, args, receiver
    let type_tag = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected type_tag string, got {:?}", other),
    };
    let method = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected method string, got {:?}", other),
    };
    let _vtable_slot = match frame.stack.pop() {
        Some(Data::Int(s)) => s,
        other => panic!("expected vtable_slot int, got {:?}", other),
    };
    let _return_ty = match frame.stack.pop() {
        Some(Data::Array(_)) => (),
        other => panic!("expected return_ty array, got {:?}", other),
    };
    assert_eq!(type_tag, "counter.Counter");

    let _handle = ctx.pop_foreign("counter.Counter")?;

    match method.as_str() {
        "inc" => {
            let prev = unsafe {
                let p = COUNTER_VALUE;
                COUNTER_VALUE = p + 1;
                p
            };
            ctx.stack_frame_mut()?.stack.push(Data::Int(prev));
        }
        "read" => {
            let cur = unsafe { COUNTER_VALUE };
            ctx.stack_frame_mut()?.stack.push(Data::Int(cur));
        }
        _ => panic!("unexpected method '{}'", method),
    }
    Ok(())
}

fn host_counter_release(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let _handle = match frame.stack.pop() {
        Some(Data::Int(h)) => h,
        other => panic!("expected handle int, got {:?}", other),
    };
    unsafe {
        FINALIZER_CALLS += 1;
    }
    Ok(())
}

const SOURCE: &str = r#"
@foreign(dispatcher="counter.invoke", finalizer="counter.release", type_tag="counter.Counter")
pub proto Counter {
    fn inc(self: ref<Counter>) -> int;
    fn read(self: ref<Counter>) -> int;
}

@intrinsic(name="counter.make")
pub fn make_counter() -> box<Counter>;

pub fn run() -> int {
    let c: box<Counter> = make_counter();
    let _x: int = c.inc();
    let _y: int = c.inc();
    c.read()
}
"#;

#[test]
fn foreign_proto_dispatch_and_finalizer() {
    reset_globals();

    let module = p7::compile(SOURCE.to_string()).expect("compile");

    let mut ctx = Context::new();
    ctx.register_host_function("counter.make".to_string(), host_make_counter);
    ctx.register_host_function("counter.invoke".to_string(), host_counter_invoke);
    ctx.register_host_function("counter.release".to_string(), host_counter_release);
    ctx.register_foreign_type("counter.Counter", Some("counter.release"));

    ctx.load_module(module);
    ctx.push_function("run", Vec::new());
    ctx.resume().expect("run");

    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(
        result,
        Data::Int(2),
        "two increments should yield read == 2"
    );

    ctx.collect_garbage();

    let finalizer_calls = unsafe { FINALIZER_CALLS };
    assert!(
        finalizer_calls >= 1,
        "expected finalizer to fire at least once after GC, got {}",
        finalizer_calls,
    );
}

#[test]
fn foreign_proto_compile_error_on_duplicate_type_tag() {
    let src = r#"
@foreign(dispatcher="d.invoke", finalizer="d.release", type_tag="dup")
pub proto A { fn m(self: ref<A>) -> int; }

@foreign(dispatcher="d.invoke", finalizer="d.release", type_tag="dup")
pub proto B { fn m(self: ref<B>) -> int; }
"#;
    let err = p7::compile(src.to_string());
    assert!(err.is_err(), "duplicate type_tag should fail compilation");
}

#[test]
fn foreign_proto_compile_error_on_missing_dispatcher() {
    let src = r#"
@foreign(finalizer="d.release", type_tag="ok")
pub proto A { fn m(self: ref<A>) -> int; }
"#;
    let err = p7::compile(src.to_string());
    assert!(err.is_err(), "missing dispatcher should fail compilation");
}

#[test]
fn foreign_proto_compile_error_on_missing_type_tag() {
    let src = r#"
@foreign(dispatcher="d.invoke", finalizer="d.release")
pub proto A { fn m(self: ref<A>) -> int; }
"#;
    let err = p7::compile(src.to_string());
    assert!(err.is_err(), "missing type_tag should fail compilation");
}
