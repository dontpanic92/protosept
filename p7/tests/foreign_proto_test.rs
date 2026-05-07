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

// Regression: methods that return `box<P>` / `ref<P>` / `&mut P` must encode
// the return type as `HostReturnTy::Foreign{type_tag}` so the host dispatcher
// (com.invoke) builds a ProtoBoxRef and not a `Data::Int(0)` (Void). Before
// the fix in `map_host_return_ty`, the wrapper types fell into the catch-all
// `_ => H::Void` arm and `welcome.p7`'s `host.games() -> box<IGameRegistry>`
// crashed with "Expected ProtoBoxRef or ProtoRefRef as receiver ... found
// Int(0)" the moment the script tried to call a method on the returned
// handle.

static HUB_RT: std::sync::Mutex<Option<(i64, String)>> = std::sync::Mutex::new(None);

fn host_hub_make(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    ctx.push_foreign("hub.Hub", 0)
}

fn host_hub_invoke(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let _type_tag = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected type_tag string, got {:?}", other),
    };
    let _method = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected method string, got {:?}", other),
    };
    let _vtable_slot = match frame.stack.pop() {
        Some(Data::Int(s)) => s,
        other => panic!("expected vtable_slot int, got {:?}", other),
    };
    // Inspect the encoded return-type marker. For `box<Other>` / `ref<Other>`
    // we expect the Foreign-tagged shape: Array([Int(4), String("other.Other")]).
    // Before the fix this was Array([Int(0)]) (Void), which is the bug we're
    // guarding against here.
    match frame.stack.pop() {
        Some(Data::Array(elems)) => {
            let mut iter = elems.into_iter();
            let tag = match iter.next() {
                Some(Data::Int(n)) => n,
                other => panic!("expected return-ty tag int, got {:?}", other),
            };
            let type_tag = match iter.next() {
                Some(Data::String(s)) => s,
                _ => String::new(),
            };
            *HUB_RT.lock().unwrap() = Some((tag, type_tag));
        }
        other => panic!("expected return_ty array, got {:?}", other),
    }
    let _handle = ctx.pop_foreign("hub.Hub")?;

    // Hub::get_other / borrow_other -> box<Other> / ref<Other>
    ctx.push_foreign("other.Other", 1)
}

fn host_other_invoke(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let _type_tag = match frame.stack.pop() {
        Some(Data::String(s)) => s,
        other => panic!("expected type_tag string, got {:?}", other),
    };
    let _method = match frame.stack.pop() {
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
    let _handle = ctx.pop_foreign("other.Other")?;
    ctx.stack_frame_mut()?.stack.push(Data::Int(42));
    Ok(())
}

fn host_hub_release(_ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    Ok(())
}
fn host_other_release(_ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    Ok(())
}

const HANDLE_RETURN_SOURCE: &str = r#"
@foreign(dispatcher="other.invoke", finalizer="other.release", type_tag="other.Other")
pub proto Other {
    fn value(self: ref<Other>) -> int;
}

@foreign(dispatcher="hub.invoke", finalizer="hub.release", type_tag="hub.Hub")
pub proto Hub {
    fn get_other(self: ref<Hub>) -> box<Other>;
    fn borrow_other(self: ref<Hub>) -> ref<Other>;
}

@intrinsic(name="hub.make")
pub fn make_hub() -> box<Hub>;

pub fn run() -> int {
    let h: box<Hub> = make_hub();
    let o: box<Other> = h.get_other();
    o.value()
}

pub fn run_ref() -> int {
    let h: box<Hub> = make_hub();
    let o: ref<Other> = h.borrow_other();
    o.value()
}
"#;

fn build_handle_return_ctx() -> Context {
    let module = p7::compile(HANDLE_RETURN_SOURCE.to_string()).expect("compile");
    let mut ctx = Context::new();
    ctx.register_host_function("hub.make".to_string(), host_hub_make);
    ctx.register_host_function("hub.invoke".to_string(), host_hub_invoke);
    ctx.register_host_function("hub.release".to_string(), host_hub_release);
    ctx.register_host_function("other.invoke".to_string(), host_other_invoke);
    ctx.register_host_function("other.release".to_string(), host_other_release);
    ctx.register_foreign_type("hub.Hub", Some("hub.release"));
    ctx.register_foreign_type("other.Other", Some("other.release"));
    ctx.load_module(module);
    ctx
}

#[test]
fn foreign_proto_method_returning_box_proto_encodes_foreign_return_ty() {
    *HUB_RT.lock().unwrap() = None;
    let mut ctx = build_handle_return_ctx();
    ctx.push_function("run", Vec::new());
    ctx.resume().expect("run");

    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(result, Data::Int(42));

    // The encoded return-ty for `get_other -> box<Other>` must be the
    // Foreign-tagged marker (tag == 4), carrying the inner proto's type_tag.
    // Tag 0 (Void) here would be the regression.
    let captured = HUB_RT.lock().unwrap().clone().expect("invoke captured rt");
    assert_eq!(
        captured.0, 4,
        "box<Other> return must encode as HostReturnTy::Foreign (tag 4), got tag {}",
        captured.0
    );
    assert_eq!(captured.1, "other.Other");
}

#[test]
fn foreign_proto_method_returning_ref_proto_encodes_foreign_return_ty() {
    *HUB_RT.lock().unwrap() = None;
    let mut ctx = build_handle_return_ctx();
    ctx.push_function("run_ref", Vec::new());
    ctx.resume().expect("run_ref");

    let result = ctx.stack[0].stack.pop().expect("result");
    assert_eq!(result, Data::Int(42));

    let captured = HUB_RT.lock().unwrap().clone().expect("invoke captured rt");
    assert_eq!(
        captured.0, 4,
        "ref<Other> return must encode as HostReturnTy::Foreign (tag 4), got tag {}",
        captured.0
    );
    assert_eq!(captured.1, "other.Other");
}
