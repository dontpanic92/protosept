use p7::embedding::{CallOutcome, Runtime};
use p7::errors::RuntimeError;
use p7::interpreter::context::Data;
use p7::native_abi::{
    P7_NATIVE_ABI_VERSION, P7CallApi, P7CallbackValue, P7CallbackValueKind, P7HostApi,
    P7NativeFunctionDescriptor, P7NativeType, P7Status, P7Value,
};
use std::ffi::{CString, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

unsafe extern "C" fn add_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    if arg_count != 2 {
        return P7Status::InvalidArgument;
    }
    // SAFETY: The runtime supplies two readable argument handles.
    let args = unsafe { std::slice::from_raw_parts(args, arg_count) };
    let mut lhs = 0;
    let mut rhs = 0;
    // SAFETY: api is valid for the duration of the callback.
    let api_ref = unsafe { &*api };
    let status = unsafe { (api_ref.get_int)(api, args[0], &mut lhs) };
    if status != P7Status::Ok {
        return status;
    }
    let status = unsafe { (api_ref.get_int)(api, args[1], &mut rhs) };
    if status != P7Status::Ok {
        return status;
    }
    unsafe { (api_ref.make_int)(api, lhs + rhs, output) }
}

unsafe extern "C" fn invoke_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    if arg_count != 2 {
        return P7Status::InvalidArgument;
    }
    // SAFETY: The runtime supplies two readable argument handles.
    let args = unsafe { std::slice::from_raw_parts(args, arg_count) };
    // SAFETY: api is valid and args[1] is a live value handle.
    unsafe { ((*api).invoke_callback)(api, args[0], &args[1], 1, output) }
}

unsafe extern "C" fn failing_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    _output: *mut P7Value,
) -> P7Status {
    let message = b"extension rejected the operation";
    // SAFETY: api is valid and message remains readable for the call.
    unsafe { ((*api).set_error)(api, message.as_ptr(), message.len()) };
    P7Status::Error
}

unsafe extern "C" fn structured_failing_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    _output: *mut P7Value,
) -> P7Status {
    let operation = b"widget.refresh";
    let class = b"EWidgetDisconnected";
    let message = b"device disappeared";
    let status = unsafe {
        ((*api).set_error_details)(
            api,
            operation.as_ptr(),
            operation.len(),
            class.as_ptr(),
            class.len(),
            message.as_ptr(),
            message.len(),
        )
    };
    if status == P7Status::Ok {
        P7Status::Error
    } else {
        status
    }
}

unsafe extern "C" fn validate_error_details_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    let invalid_utf8 = [0xff];
    let set_details = unsafe { (*api).set_error_details };
    if unsafe {
        set_details(
            api,
            std::ptr::null(),
            1,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        )
    } != P7Status::InvalidArgument
        || unsafe {
            set_details(
                api,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            )
        } != P7Status::InvalidArgument
    {
        return P7Status::Error;
    }
    unsafe { ((*api).make_int)(api, 42, output) }
}

unsafe extern "C" fn stale_error_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    if arg_count != 1 {
        return P7Status::InvalidArgument;
    }
    let mut mode = 0;
    let status = unsafe { ((*api).get_int)(api, *args, &mut mode) };
    if status != P7Status::Ok {
        return status;
    }
    if mode == 0 {
        let stale = b"must not leak";
        let status = unsafe { ((*api).set_error)(api, stale.as_ptr(), stale.len()) };
        if status != P7Status::Ok {
            return status;
        }
        unsafe { ((*api).make_int)(api, 7, output) }
    } else {
        P7Status::Error
    }
}

unsafe fn register(
    api: *const P7HostApi,
    name: &str,
    params: &[P7NativeType],
    result: Option<P7NativeType>,
    callback: p7::native_abi::P7NativeCallback,
    userdata: *mut c_void,
    drop_userdata: Option<p7::native_abi::P7DropUserdata>,
) -> P7Status {
    let name = CString::new(name).expect("native name");
    let descriptor = P7NativeFunctionDescriptor {
        struct_size: std::mem::size_of::<P7NativeFunctionDescriptor>(),
        name: name.as_ptr(),
        params: params.as_ptr(),
        param_count: params.len(),
        result: result.unwrap_or(P7NativeType::Any),
        has_result: u8::from(result.is_some()),
        callback: Some(callback),
        userdata,
        drop_userdata,
    };
    // SAFETY: api and descriptor are valid for synchronous registration.
    unsafe { ((*api).register_function)((*api).runtime, &descriptor) }
}

unsafe extern "C" fn extension_init(api: *const P7HostApi) -> P7Status {
    // SAFETY: The host supplies a valid API table.
    let api_ref = unsafe { &*api };
    if api_ref.abi_version != P7_NATIVE_ABI_VERSION
        || api_ref.struct_size < std::mem::size_of::<P7HostApi>()
    {
        return P7Status::InvalidArgument;
    }

    for status in [
        unsafe {
            register(
                api,
                "abi.add",
                &[P7NativeType::Int, P7NativeType::Int],
                Some(P7NativeType::Int),
                add_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.structured_fail",
                &[],
                Some(P7NativeType::Int),
                structured_failing_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.validate_error_details",
                &[],
                Some(P7NativeType::Int),
                validate_error_details_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.stale_error",
                &[P7NativeType::Int],
                Some(P7NativeType::Int),
                stale_error_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.invoke",
                &[P7NativeType::Closure, P7NativeType::Int],
                Some(P7NativeType::Int),
                invoke_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.fail",
                &[],
                Some(P7NativeType::Int),
                failing_callback,
                std::ptr::null_mut(),
                None,
            )
        },
    ] {
        if status != P7Status::Ok {
            return status;
        }
    }
    P7Status::Ok
}

static USERDATA_DROPS: AtomicUsize = AtomicUsize::new(0);
static USERDATA_TEST_LOCK: Mutex<()> = Mutex::new(());
static ROOTED_CALLBACK_TOKEN: Mutex<u64> = Mutex::new(0);
static ROOTED_CALLBACK_TEST_LOCK: Mutex<()> = Mutex::new(());
static ROOTED_CALLBACK_RUNTIME: Mutex<usize> = Mutex::new(0);
static ROOTED_CALLBACK_INVOKE: Mutex<Option<unsafe extern "C" fn(*mut c_void, u64) -> P7Status>> =
    Mutex::new(None);
static ROOTED_CALLBACK_RELEASE: Mutex<Option<unsafe extern "C" fn(*mut c_void, u64) -> P7Status>> =
    Mutex::new(None);
type InvokeRootedValues = unsafe extern "C" fn(
    *mut c_void,
    u64,
    *const P7CallbackValue,
    usize,
    *mut P7CallbackValue,
) -> P7Status;
static ROOTED_CALLBACK_INVOKE_VALUES: Mutex<Option<InvokeRootedValues>> = Mutex::new(None);

unsafe extern "C" fn retain_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    _output: *mut P7Value,
) -> P7Status {
    if arg_count != 1 {
        return P7Status::InvalidArgument;
    }
    let mut token = 0;
    let status = unsafe { ((*api).retain_callback)(api, *args, &mut token) };
    if status == P7Status::Ok {
        *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock") = token;
    }
    status
}

unsafe extern "C" fn invoke_rooted_from_native(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    _output: *mut P7Value,
) -> P7Status {
    let stale = b"outer error must be replaced";
    let status = unsafe { ((*api).set_error)(api, stale.as_ptr(), stale.len()) };
    if status != P7Status::Ok {
        return status;
    }
    let token = *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock");
    let runtime = *ROOTED_CALLBACK_RUNTIME
        .lock()
        .expect("callback runtime lock") as *mut c_void;
    let invoke = ROOTED_CALLBACK_INVOKE
        .lock()
        .expect("callback invoke lock")
        .expect("invoke callback function");
    unsafe { invoke(runtime, token) }
}

unsafe extern "C" fn rooted_callback_init(api: *const P7HostApi) -> P7Status {
    *ROOTED_CALLBACK_RUNTIME
        .lock()
        .expect("callback runtime lock") = unsafe { (*api).runtime } as usize;
    *ROOTED_CALLBACK_INVOKE.lock().expect("callback invoke lock") =
        Some(unsafe { (*api).invoke_rooted_callback });
    *ROOTED_CALLBACK_RELEASE
        .lock()
        .expect("callback release lock") = Some(unsafe { (*api).release_rooted_callback });
    *ROOTED_CALLBACK_INVOKE_VALUES
        .lock()
        .expect("callback values lock") = Some(unsafe { (*api).invoke_rooted_callback_values });
    for status in [
        unsafe {
            register(
                api,
                "abi.retain_callback",
                &[P7NativeType::Closure],
                None,
                retain_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.invoke_rooted",
                &[],
                None,
                invoke_rooted_from_native,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.structured_fail",
                &[],
                Some(P7NativeType::Int),
                structured_failing_callback,
                std::ptr::null_mut(),
                None,
            )
        },
    ] {
        if status != P7Status::Ok {
            return status;
        }
    }
    P7Status::Ok
}

unsafe extern "C" fn drop_userdata(userdata: *mut c_void) {
    if !userdata.is_null() {
        // SAFETY: The initializer allocated this exact Box and transfers it
        // to the runtime after successful registration.
        drop(unsafe { Box::from_raw(userdata.cast::<u64>()) });
        USERDATA_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "C" fn userdata_init(api: *const P7HostApi) -> P7Status {
    let userdata = Box::into_raw(Box::new(42_u64)).cast();
    let status = unsafe {
        register(
            api,
            "abi.userdata",
            &[],
            Some(P7NativeType::Int),
            add_userdata,
            userdata,
            Some(drop_userdata),
        )
    };
    if status != P7Status::Ok {
        unsafe { drop_userdata(userdata) };
    }
    status
}

unsafe extern "C" fn failing_init(api: *const P7HostApi) -> P7Status {
    let userdata = Box::into_raw(Box::new(42_u64)).cast();
    let status = unsafe {
        register(
            api,
            "abi.partial",
            &[],
            Some(P7NativeType::Int),
            add_userdata,
            userdata,
            Some(drop_userdata),
        )
    };
    if status != P7Status::Ok {
        unsafe { drop_userdata(userdata) };
        return status;
    }
    P7Status::Error
}

unsafe extern "C" fn add_userdata(
    userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    // SAFETY: userdata points to the u64 allocated by userdata_init.
    let value = unsafe { *userdata.cast::<u64>() } as i64;
    unsafe { ((*api).make_int)(api, value, output) }
}

unsafe extern "C" fn make_foreign_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    let type_tag = b"abi.Widget";
    unsafe { ((*api).make_foreign_handle)(api, type_tag.as_ptr(), type_tag.len(), 42, output) }
}

unsafe extern "C" fn read_foreign_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    if arg_count != 1 {
        return P7Status::InvalidArgument;
    }
    let type_tag = b"abi.Widget";
    let mut handle = 0;
    let status =
        unsafe { ((*api).get_foreign)(api, *args, type_tag.as_ptr(), type_tag.len(), &mut handle) };
    if status != P7Status::Ok {
        return status;
    }
    unsafe { ((*api).make_int)(api, handle, output) }
}

unsafe extern "C" fn foreign_init(api: *const P7HostApi) -> P7Status {
    let type_tag = CString::new("abi.Widget").expect("type tag");
    let status = unsafe {
        ((*api).register_foreign_type)((*api).runtime, type_tag.as_ptr(), std::ptr::null())
    };
    if status != P7Status::Ok {
        return status;
    }
    for status in [
        unsafe {
            register(
                api,
                "abi.make_foreign",
                &[],
                Some(P7NativeType::Foreign),
                make_foreign_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.read_foreign",
                &[P7NativeType::Foreign],
                Some(P7NativeType::Int),
                read_foreign_callback,
                std::ptr::null_mut(),
                None,
            )
        },
    ] {
        if status != P7Status::Ok {
            return status;
        }
    }
    P7Status::Ok
}

unsafe extern "C" fn make_derived_foreign_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    _args: *const P7Value,
    _arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    let type_tag = b"abi.Derived";
    unsafe { ((*api).make_foreign_handle)(api, type_tag.as_ptr(), type_tag.len(), 84, output) }
}

unsafe extern "C" fn read_base_foreign_callback(
    _userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    if arg_count != 1 {
        return P7Status::InvalidArgument;
    }
    let type_tag = b"abi.Base";
    let mut handle = 0;
    let status =
        unsafe { ((*api).get_foreign)(api, *args, type_tag.as_ptr(), type_tag.len(), &mut handle) };
    if status != P7Status::Ok {
        return status;
    }
    unsafe { ((*api).make_int)(api, handle, output) }
}

unsafe extern "C" fn foreign_inheritance_init(api: *const P7HostApi) -> P7Status {
    for type_tag in ["abi.Base", "abi.Derived"] {
        let type_tag = CString::new(type_tag).expect("type tag");
        let status = unsafe {
            ((*api).register_foreign_type)((*api).runtime, type_tag.as_ptr(), std::ptr::null())
        };
        if status != P7Status::Ok {
            return status;
        }
    }
    for status in [
        unsafe {
            register(
                api,
                "abi.make_derived_foreign",
                &[],
                Some(P7NativeType::Foreign),
                make_derived_foreign_callback,
                std::ptr::null_mut(),
                None,
            )
        },
        unsafe {
            register(
                api,
                "abi.read_base_foreign",
                &[P7NativeType::Foreign],
                Some(P7NativeType::Int),
                read_base_foreign_callback,
                std::ptr::null_mut(),
                None,
            )
        },
    ] {
        if status != P7Status::Ok {
            return status;
        }
    }
    P7Status::Ok
}

fn compile(source: &str) -> p7::bytecode::Module {
    p7::compile(source.to_string()).expect("compile")
}

#[test]
fn extension_registers_typed_functions_and_callbacks() {
    let module = compile(
        r#"
@intrinsic(name="abi.add")
fn add(a: int, b: int) -> int;

@intrinsic(name="abi.invoke")
fn invoke(callback: fn(int) -> int, value: int) -> int;

fn run() -> int {
    let callback = (value: int) => value * 2;
    add(10, invoke(callback, 16))
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Returned(Some(Data::Int(value))) => assert_eq!(value, 42),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn extension_can_retain_invoke_and_release_callback() {
    let _guard = ROOTED_CALLBACK_TEST_LOCK
        .lock()
        .expect("callback test lock");
    *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock") = 0;
    let module = compile(
        r#"
@intrinsic(name="abi.retain_callback")
fn retain_callback(callback: fn());

let calls: box<array<int>> = box([0]);

fn install() {
    retain_callback(() => calls.push(1))
}

fn call_count() -> int {
    calls.len()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(rooted_callback_init)
        .expect("register extension");
    runtime.load_module(module);
    assert!(matches!(
        runtime.call("install", Vec::new()).expect("install"),
        CallOutcome::Returned(_)
    ));

    let token = *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock");
    let runtime_ptr = *ROOTED_CALLBACK_RUNTIME
        .lock()
        .expect("callback runtime lock") as *mut c_void;
    assert_ne!(token, 0);
    let invoke = ROOTED_CALLBACK_INVOKE
        .lock()
        .expect("callback invoke lock")
        .expect("invoke callback function");
    let release = ROOTED_CALLBACK_RELEASE
        .lock()
        .expect("callback release lock")
        .expect("release callback function");
    assert_eq!(unsafe { invoke(runtime_ptr, token) }, P7Status::Ok);
    assert!(matches!(
        runtime.call("call_count", Vec::new()).expect("count"),
        CallOutcome::Returned(Some(Data::Int(2)))
    ));
    assert_eq!(unsafe { release(runtime_ptr, token) }, P7Status::Ok);
    assert_eq!(unsafe { invoke(runtime_ptr, token) }, P7Status::Error);
}

#[test]
fn extension_reads_validated_foreign_values() {
    let module = compile(
        r#"
@foreign(type_tag="abi.Widget", dispatcher="abi.unused")
proto Widget {
}

@intrinsic(name="abi.make_foreign")
fn make_foreign() -> handle<Widget>;

@intrinsic(name="abi.read_foreign")
fn read_foreign(value: handle<Widget>) -> int;

fn run() -> int {
    read_foreign(make_foreign())
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(foreign_init)
        .expect("register extension");
    runtime.load_module(module);

    assert!(matches!(
        runtime.call("run", Vec::new()).expect("run"),
        CallOutcome::Returned(Some(Data::Int(42)))
    ));
}

#[test]
fn native_get_foreign_accepts_derived_value_for_base_tag() {
    let module = compile(
        r#"
@foreign(type_tag="abi.Base", dispatcher="abi.unused")
proto Base {}

@foreign(type_tag="abi.Derived", dispatcher="abi.unused")
proto[Base] Derived {}

@intrinsic(name="abi.make_derived_foreign")
fn make_derived_foreign() -> handle<Derived>;

@intrinsic(name="abi.read_base_foreign")
fn read_base_foreign(value: handle<Base>) -> int;

fn run() -> int {
    read_base_foreign(make_derived_foreign())
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(foreign_inheritance_init)
        .expect("register extension");
    runtime.load_module(module);

    assert!(matches!(
        runtime.call("run", Vec::new()).expect("run"),
        CallOutcome::Returned(Some(Data::Int(84)))
    ));
}

#[test]
fn source_handle_type_becomes_stale_after_host_invalidation() {
    let module = compile(
        r#"
@foreign(type_tag="abi.Widget", dispatcher="abi.unused")
proto Widget {
}

@intrinsic(name="abi.make_foreign")
fn make_foreign() -> handle<Widget>;

@intrinsic(name="abi.read_foreign")
fn read_foreign(value: handle<Widget>) -> int;

let widget: handle<Widget> = make_foreign();

fn run() -> int {
    read_foreign(widget)
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(foreign_init)
        .expect("register extension");
    runtime.load_module(module);
    runtime
        .context_mut()
        .invalidate_foreign_handle("abi.Widget", 42)
        .expect("invalidate handle");

    assert!(matches!(
        runtime.call("run", Vec::new()).expect("run"),
        CallOutcome::Trapped(_)
    ));
}

#[test]
fn extension_error_message_becomes_a_runtime_trap() {
    let module = compile(
        r#"
@intrinsic(name="abi.fail")
fn fail() -> int;

fn run() -> int {
    fail()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Trapped(RuntimeError::Native(error)) => {
            assert_eq!(error.operation, "");
            assert_eq!(error.class, "");
            assert_eq!(error.message, "extension rejected the operation");
        }

        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn structured_error_details_become_a_runtime_trap() {
    let module = compile(
        r#"
@intrinsic(name="abi.structured_fail")
fn structured_fail() -> int;

fn run() -> int {
    structured_fail()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Trapped(RuntimeError::Native(error)) => {
            assert_eq!(error.operation, "widget.refresh");
            assert_eq!(error.class, "EWidgetDisconnected");
            assert_eq!(error.message, "device disappeared");
            assert_eq!(
                error.to_string(),
                "Native error during 'widget.refresh' (EWidgetDisconnected): device disappeared"
            );
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn error_details_reject_invalid_arguments_without_poisoning_the_call() {
    let module = compile(
        r#"
@intrinsic(name="abi.validate_error_details")
fn validate_error_details() -> int;

fn run() -> int {
    validate_error_details()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    assert!(matches!(
        runtime.call("run", Vec::new()).expect("run"),
        CallOutcome::Returned(Some(Data::Int(42)))
    ));
}

#[test]
fn native_error_state_is_cleared_between_invocations() {
    let module = compile(
        r#"
@intrinsic(name="abi.stale_error")
fn stale_error(mode: int) -> int;

fn prime() -> int {
    stale_error(0)
}

fn fail() -> int {
    stale_error(1)
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    assert!(matches!(
        runtime.call("prime", Vec::new()).expect("prime"),
        CallOutcome::Returned(Some(Data::Int(7)))
    ));
    match runtime.call("fail", Vec::new()).expect("fail") {
        CallOutcome::Trapped(RuntimeError::Native(error)) => {
            assert_eq!(error.operation, "abi.stale_error");
            assert_eq!(error.class, "");
            assert_eq!(error.message, "status Error");
            assert!(!error.to_string().contains("must not leak"));
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn nested_callback_preserves_structured_native_error() {
    let module = compile(
        r#"
@intrinsic(name="abi.invoke")
fn invoke(callback: fn(int) -> int, value: int) -> int;

@intrinsic(name="abi.structured_fail")
fn structured_fail() -> int;

fn run() -> int {
    invoke((value: int) => structured_fail(), 0)
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(extension_init)
        .expect("register extension");
    runtime.load_module(module);

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Trapped(RuntimeError::Native(error)) => {
            assert_eq!(error.operation, "widget.refresh");
            assert_eq!(error.class, "EWidgetDisconnected");
            assert_eq!(error.message, "device disappeared");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn rooted_callback_reentry_preserves_structured_native_error() {
    let _guard = ROOTED_CALLBACK_TEST_LOCK
        .lock()
        .expect("callback test lock");
    *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock") = 0;
    let module = compile(
        r#"
@intrinsic(name="abi.retain_callback")
fn retain_callback(callback: fn());

@intrinsic(name="abi.invoke_rooted")
fn invoke_rooted();

@intrinsic(name="abi.structured_fail")
fn structured_fail() -> int;

fn install() {
    retain_callback(() => {
        structured_fail();
    })
}

fn run() {
    invoke_rooted()
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(rooted_callback_init)
        .expect("register extension");
    runtime.load_module(module);
    runtime.call("install", Vec::new()).expect("install");

    match runtime.call("run", Vec::new()).expect("run") {
        CallOutcome::Trapped(RuntimeError::Native(error)) => {
            assert_eq!(error.operation, "widget.refresh");
            assert_eq!(error.class, "EWidgetDisconnected");
            assert_eq!(error.message, "device disappeared");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn structured_error_setter_is_the_last_call_api_field() {
    let field_end = std::mem::offset_of!(P7CallApi, set_error_details)
        + std::mem::size_of::<
            unsafe extern "C" fn(
                *const P7CallApi,
                *const u8,
                usize,
                *const u8,
                usize,
                *const u8,
                usize,
            ) -> P7Status,
        >();
    assert_eq!(field_end, std::mem::size_of::<P7CallApi>());
    assert!(
        std::mem::offset_of!(P7CallApi, set_error_details)
            > std::mem::offset_of!(P7CallApi, runtime)
    );
}

#[test]
fn extension_can_invoke_rooted_callback_with_typed_values() {
    let _guard = ROOTED_CALLBACK_TEST_LOCK
        .lock()
        .expect("callback test lock");
    *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock") = 0;
    let module = compile(
        r#"
@intrinsic(name="abi.retain_callback")
fn retain_callback(callback: fn(int) -> int);

fn install() {
    retain_callback((value: int) => value + 3)
}
"#,
    );
    let mut runtime = Runtime::new();
    runtime
        .register_native_extension(rooted_callback_init)
        .expect("register extension");
    runtime.load_module(module);
    runtime.call("install", Vec::new()).expect("install");

    let token = *ROOTED_CALLBACK_TOKEN.lock().expect("callback token lock");
    let runtime_ptr = *ROOTED_CALLBACK_RUNTIME
        .lock()
        .expect("callback runtime lock") as *mut c_void;
    let invoke = ROOTED_CALLBACK_INVOKE_VALUES
        .lock()
        .expect("callback values lock")
        .expect("typed invoke callback function");
    let input = P7CallbackValue {
        kind: P7CallbackValueKind::Int as u32,
        int_value: 39,
        float_value: 0.0,
        bytes: std::ptr::null(),
        length: 0,
    };
    let mut output = P7CallbackValue {
        kind: P7CallbackValueKind::Unit as u32,
        int_value: 0,
        float_value: 0.0,
        bytes: std::ptr::null(),
        length: 0,
    };
    assert_eq!(
        unsafe { invoke(runtime_ptr, token, &input, 1, &mut output) },
        P7Status::Ok
    );
    assert_eq!(output.kind, P7CallbackValueKind::Int as u32);
    assert_eq!(output.int_value, 42);

    let invalid = P7CallbackValue {
        kind: u32::MAX,
        ..input
    };
    assert_eq!(
        unsafe { invoke(runtime_ptr, token, &invalid, 1, &mut output) },
        P7Status::TypeMismatch
    );
}

#[test]
fn extension_userdata_is_released_with_the_runtime() {
    let _guard = USERDATA_TEST_LOCK.lock().expect("userdata test lock");
    USERDATA_DROPS.store(0, Ordering::SeqCst);
    {
        let mut runtime = Runtime::new();
        runtime
            .register_native_extension(userdata_init)
            .expect("register extension");
        runtime.load_module(compile(
            r#"
@intrinsic(name="abi.userdata")
fn userdata() -> int;

fn run() -> int {
    userdata()
}
"#,
        ));
        assert!(matches!(
            runtime.call("run", Vec::new()).expect("run"),
            CallOutcome::Returned(Some(Data::Int(42)))
        ));
    }
    assert_eq!(USERDATA_DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_initializer_rolls_back_registrations_and_userdata() {
    let _guard = USERDATA_TEST_LOCK.lock().expect("userdata test lock");
    USERDATA_DROPS.store(0, Ordering::SeqCst);
    let mut runtime = Runtime::new();
    let error = runtime
        .register_native_extension(failing_init)
        .expect_err("initializer should fail");
    assert!(error.to_string().contains("status Error"));
    assert_eq!(USERDATA_DROPS.load(Ordering::SeqCst), 1);

    runtime.load_module(compile(
        r#"
@intrinsic(name="abi.partial")
fn partial() -> int;

fn run() -> int {
    partial()
}
"#,
    ));
    assert!(matches!(
        runtime.call("run", Vec::new()).expect("run"),
        CallOutcome::Trapped(_)
    ));
}
