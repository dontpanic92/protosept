use std::ffi::{c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;

const ABI_VERSION: u32 = 1;

#[cfg(fixture_first)]
const LABEL: &str = "first";
#[cfg(fixture_second)]
const LABEL: &str = "second";
#[cfg(fixture_failure)]
const LABEL: &str = "failure";

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok = 0,
    Error = 1,
    InvalidArgument = 2,
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum NativeType {
    Any = 0,
}

#[repr(C)]
struct FunctionDescriptor {
    struct_size: usize,
    name: *const c_char,
    params: *const NativeType,
    param_count: usize,
    result: NativeType,
    has_result: u8,
    callback: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_void,
            usize,
            *mut c_void,
        ) -> Status,
    >,
    userdata: *mut c_void,
    drop_userdata: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
struct HostApi {
    abi_version: u32,
    struct_size: usize,
    runtime: *mut c_void,
    register_function: unsafe extern "C" fn(*mut c_void, *const FunctionDescriptor) -> Status,
}

fn record(event: &str) {
    let path = std::env::var_os("P7_SHUTDOWN_LOG").expect("P7_SHUTDOWN_LOG");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open shutdown log");
    writeln!(file, "{event}:{LABEL}").expect("write shutdown log");
}

unsafe extern "C" fn callback(
    _userdata: *mut c_void,
    _api: *const c_void,
    _args: *const c_void,
    _arg_count: usize,
    _output: *mut c_void,
) -> Status {
    Status::Ok
}

unsafe extern "C" fn drop_userdata(_userdata: *mut c_void) {
    record("drop");
}

#[no_mangle]
unsafe extern "C" fn p7_extension_init_v1(api: *const HostApi) -> Status {
    if api.is_null() || unsafe { (*api).abi_version } != ABI_VERSION {
        return Status::InvalidArgument;
    }
    let descriptor = FunctionDescriptor {
        struct_size: std::mem::size_of::<FunctionDescriptor>(),
        name: c"dynamic.lifecycle".as_ptr(),
        params: std::ptr::null(),
        param_count: 0,
        result: NativeType::Any,
        has_result: 0,
        callback: Some(callback),
        userdata: std::ptr::null_mut(),
        drop_userdata: Some(drop_userdata),
    };
    unsafe { ((*api).register_function)((*api).runtime, &descriptor) }
}

#[no_mangle]
unsafe extern "C" fn p7_extension_shutdown_v1(api: *const HostApi) -> Status {
    if api.is_null() || unsafe { (*api).abi_version } != ABI_VERSION {
        return Status::InvalidArgument;
    }
    record("shutdown");
    #[cfg(fixture_failure)]
    {
        return Status::Error;
    }
    #[cfg(not(fixture_failure))]
    Status::Ok
}
