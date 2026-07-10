use std::ffi::{c_char, c_void};

const ABI_VERSION: u32 = 1;

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok = 0,
    InvalidArgument = 2,
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum NativeType {
    Any = 0,
    Int = 1,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Value(u64);

#[repr(C)]
struct CallApi {
    abi_version: u32,
    struct_size: usize,
    context: *mut c_void,
    value_kind: *const c_void,
    get_int: *const c_void,
    get_float: *const c_void,
    get_bool: *const c_void,
    copy_string: *const c_void,
    make_int: unsafe extern "C" fn(*const CallApi, i64, *mut Value) -> Status,
}

type Callback = unsafe extern "C" fn(
    *mut c_void,
    *const CallApi,
    *const Value,
    usize,
    *mut Value,
) -> Status;

#[repr(C)]
struct FunctionDescriptor {
    struct_size: usize,
    name: *const c_char,
    params: *const NativeType,
    param_count: usize,
    result: NativeType,
    has_result: u8,
    callback: Option<Callback>,
    userdata: *mut c_void,
    drop_userdata: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
struct HostApi {
    abi_version: u32,
    struct_size: usize,
    runtime: *mut c_void,
    register_function:
        unsafe extern "C" fn(*mut c_void, *const FunctionDescriptor) -> Status,
    register_foreign_type: *const c_void,
    invalidate_foreign_handle: *const c_void,
}

unsafe extern "C" fn answer(
    _userdata: *mut c_void,
    api: *const CallApi,
    _args: *const Value,
    _arg_count: usize,
    output: *mut Value,
) -> Status {
    unsafe { ((*api).make_int)(api, 42, output) }
}

#[no_mangle]
unsafe extern "C" fn p7_extension_init_v1(api: *const HostApi) -> Status {
    if api.is_null() || unsafe { (*api).abi_version } != ABI_VERSION {
        return Status::InvalidArgument;
    }
    let descriptor = FunctionDescriptor {
        struct_size: std::mem::size_of::<FunctionDescriptor>(),
        name: c"dynamic.answer".as_ptr(),
        params: std::ptr::null(),
        param_count: 0,
        result: NativeType::Int,
        has_result: 1,
        callback: Some(answer),
        userdata: std::ptr::null_mut(),
        drop_userdata: None,
    };
    unsafe { ((*api).register_function)((*api).runtime, &descriptor) }
}
