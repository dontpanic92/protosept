use p7::embedding::{CallOutcome, Runtime};
use p7::interpreter::context::Data;
use p7::native_abi::{
    P7_NATIVE_ABI_VERSION, P7CallApi, P7HostApi, P7NativeFunctionDescriptor, P7NativeType,
    P7Status, P7Value,
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
        CallOutcome::Trapped(error) => {
            assert!(
                error
                    .to_string()
                    .contains("extension rejected the operation")
            );
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
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
