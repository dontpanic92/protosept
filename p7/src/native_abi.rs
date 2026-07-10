use crate::errors::RuntimeError;
use crate::interpreter::context::{Context, Data};
use crate::interpreter::native::{NativeSignature, NativeType};
use libloading::Library;
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr;
use std::rc::Rc;
use std::slice;

pub const P7_NATIVE_ABI_VERSION: u32 = 1;
pub const P7_EXTENSION_INIT_SYMBOL: &[u8] = b"p7_extension_init_v1\0";

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P7Status {
    Ok = 0,
    Error = 1,
    InvalidArgument = 2,
    TypeMismatch = 3,
    StaleHandle = 4,
    Panic = 5,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P7NativeType {
    Any = 0,
    Int = 1,
    Float = 2,
    Bool = 3,
    String = 4,
    Array = 5,
    Tuple = 6,
    Map = 7,
    Closure = 8,
    Foreign = 9,
}

impl From<P7NativeType> for NativeType {
    fn from(value: P7NativeType) -> Self {
        match value {
            P7NativeType::Any => Self::Any,
            P7NativeType::Int => Self::Int,
            P7NativeType::Float => Self::Float,
            P7NativeType::Bool => Self::Bool,
            P7NativeType::String => Self::String,
            P7NativeType::Array => Self::Array,
            P7NativeType::Tuple => Self::Tuple,
            P7NativeType::Map => Self::Map,
            P7NativeType::Closure => Self::Closure,
            P7NativeType::Foreign => Self::Foreign,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P7ValueKind {
    Invalid = 0,
    Int = 1,
    Float = 2,
    String = 3,
    Array = 4,
    Tuple = 5,
    Map = 6,
    Closure = 7,
    Foreign = 8,
    Null = 9,
    Other = 10,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct P7Value(pub u64);

pub type P7NativeCallback = unsafe extern "C" fn(
    userdata: *mut c_void,
    api: *const P7CallApi,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status;

pub type P7DropUserdata = unsafe extern "C" fn(userdata: *mut c_void);

#[repr(C)]
pub struct P7NativeFunctionDescriptor {
    pub struct_size: usize,
    pub name: *const c_char,
    pub params: *const P7NativeType,
    pub param_count: usize,
    pub result: P7NativeType,
    pub has_result: u8,
    pub callback: Option<P7NativeCallback>,
    pub userdata: *mut c_void,
    pub drop_userdata: Option<P7DropUserdata>,
}

#[repr(C)]
pub struct P7HostApi {
    pub abi_version: u32,
    pub struct_size: usize,
    pub runtime: *mut c_void,
    pub register_function:
        unsafe extern "C" fn(*mut c_void, *const P7NativeFunctionDescriptor) -> P7Status,
    pub register_foreign_type:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> P7Status,
    pub invalidate_foreign_handle:
        unsafe extern "C" fn(*mut c_void, *const u8, usize, i64) -> P7Status,
}

#[repr(C)]
pub struct P7CallApi {
    pub abi_version: u32,
    pub struct_size: usize,
    pub context: *mut c_void,
    pub value_kind: unsafe extern "C" fn(*const P7CallApi, P7Value) -> P7ValueKind,
    pub get_int: unsafe extern "C" fn(*const P7CallApi, P7Value, *mut i64) -> P7Status,
    pub get_float: unsafe extern "C" fn(*const P7CallApi, P7Value, *mut f64) -> P7Status,
    pub get_bool: unsafe extern "C" fn(*const P7CallApi, P7Value, *mut u8) -> P7Status,
    pub copy_string:
        unsafe extern "C" fn(*const P7CallApi, P7Value, *mut u8, usize, *mut usize) -> P7Status,
    pub make_int: unsafe extern "C" fn(*const P7CallApi, i64, *mut P7Value) -> P7Status,
    pub make_float: unsafe extern "C" fn(*const P7CallApi, f64, *mut P7Value) -> P7Status,
    pub make_bool: unsafe extern "C" fn(*const P7CallApi, u8, *mut P7Value) -> P7Status,
    pub make_string:
        unsafe extern "C" fn(*const P7CallApi, *const u8, usize, *mut P7Value) -> P7Status,
    pub make_foreign_owned:
        unsafe extern "C" fn(*const P7CallApi, *const u8, usize, i64, *mut P7Value) -> P7Status,
    pub make_foreign_ref:
        unsafe extern "C" fn(*const P7CallApi, *const u8, usize, i64, *mut P7Value) -> P7Status,
    pub make_foreign_handle:
        unsafe extern "C" fn(*const P7CallApi, *const u8, usize, i64, *mut P7Value) -> P7Status,
    pub invalidate_foreign_handle:
        unsafe extern "C" fn(*const P7CallApi, *const u8, usize, i64) -> P7Status,
    pub invoke_callback: unsafe extern "C" fn(
        *const P7CallApi,
        P7Value,
        *const P7Value,
        usize,
        *mut P7Value,
    ) -> P7Status,
    pub set_error: unsafe extern "C" fn(*const P7CallApi, *const u8, usize) -> P7Status,
    pub get_foreign:
        unsafe extern "C" fn(*const P7CallApi, P7Value, *const u8, usize, *mut i64) -> P7Status,
}

pub type P7ExtensionInit = unsafe extern "C" fn(*const P7HostApi) -> P7Status;

pub struct NativeExtension {
    _library: Library,
}

impl NativeExtension {
    pub fn load(context: &mut Context, path: &Path) -> Result<Self, RuntimeError> {
        // SAFETY: The library is retained by the returned NativeExtension for
        // at least as long as every callback registered from it.
        let library = unsafe { Library::new(path) }.map_err(|error| {
            RuntimeError::Other(format!(
                "Cannot load native extension '{}': {error}",
                path.display()
            ))
        })?;
        // SAFETY: The symbol name and function signature are the versioned
        // native ABI contract. A mismatched extension must not claim v1.
        let initializer = unsafe {
            *library
                .get::<P7ExtensionInit>(P7_EXTENSION_INIT_SYMBOL)
                .map_err(|error| {
                    RuntimeError::Other(format!(
                        "Native extension '{}' does not export p7_extension_init_v1: {error}",
                        path.display()
                    ))
                })?
        };
        register_initializer(context, initializer)?;
        Ok(Self { _library: library })
    }
}

pub fn register_initializer(
    context: &mut Context,
    initializer: P7ExtensionInit,
) -> Result<(), RuntimeError> {
    let checkpoint = context.native_registration_checkpoint();
    let api = P7HostApi {
        abi_version: P7_NATIVE_ABI_VERSION,
        struct_size: std::mem::size_of::<P7HostApi>(),
        runtime: (context as *mut Context).cast(),
        register_function,
        register_foreign_type,
        invalidate_foreign_handle: invalidate_runtime_foreign_handle,
    };
    let status = match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: The initializer receives a valid API table for this call.
        unsafe { initializer(&api) }
    })) {
        Ok(status) => status,
        Err(_) => {
            context.rollback_native_registration(checkpoint);
            return Err(RuntimeError::Other(
                "Native extension initializer panicked".to_string(),
            ));
        }
    };
    if let Err(error) = status_result(status, "Native extension initializer") {
        context.rollback_native_registration(checkpoint);
        return Err(error);
    }
    Ok(())
}

struct Userdata {
    value: *mut c_void,
    drop: Option<P7DropUserdata>,
}

impl Drop for Userdata {
    fn drop(&mut self) {
        if let Some(drop) = self.drop {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: The extension supplied this destructor for this
                // userdata pointer during successful registration.
                unsafe { drop(self.value) };
            }));
        }
    }
}

unsafe extern "C" fn register_function(
    runtime: *mut c_void,
    descriptor: *const P7NativeFunctionDescriptor,
) -> P7Status {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() || descriptor.is_null() {
            return Err(P7Status::InvalidArgument);
        }
        // SAFETY: Null was rejected and the pointer is valid for this call.
        let descriptor = unsafe { &*descriptor };
        if descriptor.struct_size < std::mem::size_of::<P7NativeFunctionDescriptor>() {
            return Err(P7Status::InvalidArgument);
        }
        let callback = descriptor.callback.ok_or(P7Status::InvalidArgument)?;
        let name = c_string(descriptor.name).ok_or(P7Status::InvalidArgument)?;
        let params = if descriptor.param_count == 0 {
            Vec::new()
        } else {
            if descriptor.params.is_null() {
                return Err(P7Status::InvalidArgument);
            }
            // SAFETY: The extension guarantees param_count readable elements
            // for the duration of registration.
            unsafe { slice::from_raw_parts(descriptor.params, descriptor.param_count) }
                .iter()
                .copied()
                .map(NativeType::from)
                .collect()
        };
        let result_type = (descriptor.has_result != 0).then(|| descriptor.result.into());
        let userdata = Rc::new(Userdata {
            value: descriptor.userdata,
            drop: descriptor.drop_userdata,
        });
        let signature = NativeSignature::new(params, result_type);
        // SAFETY: runtime points at the Context used to construct P7HostApi.
        let context = unsafe { &mut *runtime.cast::<Context>() };
        context.register_native_function(name, signature, move |context, args| {
            invoke_native(callback, userdata.clone(), context, args)
        });
        Ok(())
    }));
    match result {
        Ok(Ok(())) => P7Status::Ok,
        Ok(Err(status)) => status,
        Err(_) => P7Status::Panic,
    }
}

unsafe extern "C" fn register_foreign_type(
    runtime: *mut c_void,
    type_tag: *const c_char,
    finalizer: *const c_char,
) -> P7Status {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return Err(P7Status::InvalidArgument);
        }

        let type_tag = c_string(type_tag).ok_or(P7Status::InvalidArgument)?;
        let finalizer = if finalizer.is_null() {
            None
        } else {
            Some(c_string(finalizer).ok_or(P7Status::InvalidArgument)?)
        };
        // SAFETY: runtime points at the Context used to construct P7HostApi.
        let context = unsafe { &mut *runtime.cast::<Context>() };
        context.register_foreign_type(&type_tag, finalizer.as_deref());
        Ok(())
    }));
    match result {
        Ok(Ok(())) => P7Status::Ok,
        Ok(Err(status)) => status,
        Err(_) => P7Status::Panic,
    }
}

unsafe extern "C" fn invalidate_runtime_foreign_handle(
    runtime: *mut c_void,
    type_tag: *const u8,
    type_tag_len: usize,
    host_handle: i64,
) -> P7Status {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return Err(P7Status::InvalidArgument);
        }
        let type_tag = bytes_string(type_tag, type_tag_len).ok_or(P7Status::InvalidArgument)?;
        // SAFETY: runtime points at the live Context exposed by P7HostApi.
        let context = unsafe { &mut *runtime.cast::<Context>() };
        match context.invalidate_foreign_handle(type_tag, host_handle) {
            Ok(()) => Ok(()),
            Err(RuntimeError::StaleForeignHandle { .. }) => Err(P7Status::StaleHandle),
            Err(_) => Err(P7Status::Error),
        }
    }));
    match result {
        Ok(Ok(())) => P7Status::Ok,
        Ok(Err(status)) => status,
        Err(_) => P7Status::Panic,
    }
}

fn invoke_native(
    callback: P7NativeCallback,
    userdata: Rc<Userdata>,
    context: &mut Context,
    args: &[Data],
) -> Result<Option<Data>, RuntimeError> {
    let mut bridge = CallBridge::new(context, args);
    let api = bridge.api();
    let handles = (0..args.len())
        .map(|index| P7Value((index + 1) as u64))
        .collect::<Vec<_>>();
    let mut output = P7Value(0);
    let status = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: The API and argument handles remain valid for this callback.
        unsafe {
            callback(
                userdata.value,
                &api,
                handles.as_ptr(),
                handles.len(),
                &mut output,
            )
        }
    }))
    .map_err(|_| RuntimeError::Other("Native extension callback panicked".to_string()))?;
    if status != P7Status::Ok {
        let detail = bridge
            .error
            .take()
            .unwrap_or_else(|| format!("status {status:?}"));
        return Err(RuntimeError::Other(format!(
            "Native extension callback failed: {detail}"
        )));
    }
    if output.0 == 0 {
        Ok(None)
    } else {
        bridge.get(output).cloned().map(Some).ok_or_else(|| {
            RuntimeError::Other("Native callback returned invalid value".to_string())
        })
    }
}

struct CallBridge {
    context: *mut Context,
    values: Vec<Data>,
    error: Option<String>,
}

impl CallBridge {
    fn new(context: &mut Context, args: &[Data]) -> Self {
        Self {
            context,
            values: args.to_vec(),
            error: None,
        }
    }

    fn api(&mut self) -> P7CallApi {
        P7CallApi {
            abi_version: P7_NATIVE_ABI_VERSION,
            struct_size: std::mem::size_of::<P7CallApi>(),
            context: (self as *mut Self).cast(),
            value_kind,
            get_int,
            get_float,
            get_bool,
            copy_string,
            make_int,
            make_float,
            make_bool,
            make_string,
            make_foreign_owned,
            make_foreign_ref,
            make_foreign_handle,
            invalidate_foreign_handle,
            invoke_callback,
            set_error,
            get_foreign,
        }
    }

    fn get(&self, value: P7Value) -> Option<&Data> {
        value
            .0
            .checked_sub(1)
            .and_then(|index| self.values.get(index as usize))
    }

    fn push(&mut self, value: Data) -> P7Value {
        self.values.push(value);
        P7Value(self.values.len() as u64)
    }

    fn context(&mut self) -> &mut Context {
        // SAFETY: CallBridge never outlives the native invocation and has
        // exclusive access to the callback's Context.
        unsafe { &mut *self.context }
    }
}

unsafe fn bridge<'a>(api: *const P7CallApi) -> Option<&'a mut CallBridge> {
    if api.is_null() {
        return None;
    }
    // SAFETY: The caller supplied the API table created by CallBridge::api.
    let context = unsafe { (*api).context };
    if context.is_null() {
        None
    } else {
        // SAFETY: context points to the live CallBridge for this callback.
        Some(unsafe { &mut *context.cast::<CallBridge>() })
    }
}

unsafe extern "C" fn value_kind(api: *const P7CallApi, value: P7Value) -> P7ValueKind {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7ValueKind::Invalid;
        };
        match bridge.get(value) {
            Some(Data::Int(_)) => P7ValueKind::Int,
            Some(Data::Float(_)) => P7ValueKind::Float,
            Some(Data::String(_)) => P7ValueKind::String,
            Some(Data::Array(_)) => P7ValueKind::Array,
            Some(Data::Tuple(_)) => P7ValueKind::Tuple,
            Some(Data::Map(_)) => P7ValueKind::Map,
            Some(Data::Closure { .. }) => P7ValueKind::Closure,
            Some(Data::Foreign { .. })
            | Some(Data::BoxRef { .. })
            | Some(Data::ProtoBoxRef { .. })
            | Some(Data::ProtoRefRef { .. }) => P7ValueKind::Foreign,
            Some(Data::Null) => P7ValueKind::Null,
            Some(_) => P7ValueKind::Other,
            None => P7ValueKind::Invalid,
        }
    }))
    .unwrap_or(P7ValueKind::Invalid)
}

fn catch_status(callback: impl FnOnce() -> P7Status) -> P7Status {
    catch_unwind(AssertUnwindSafe(callback)).unwrap_or(P7Status::Panic)
}

macro_rules! getter {
    ($name:ident, $ty:ty, $pattern:pat => $value:expr) => {
        unsafe extern "C" fn $name(
            api: *const P7CallApi,
            handle: P7Value,
            output: *mut $ty,
        ) -> P7Status {
            catch_status(|| {
                if output.is_null() {
                    return P7Status::InvalidArgument;
                }
                let Some(bridge) = (unsafe { bridge(api) }) else {
                    return P7Status::InvalidArgument;
                };
                match bridge.get(handle) {
                    Some($pattern) => {
                        // SAFETY: output was checked for null.
                        unsafe { *output = $value };
                        P7Status::Ok
                    }
                    Some(_) => P7Status::TypeMismatch,
                    None => P7Status::InvalidArgument,
                }
            })
        }
    };
}

getter!(get_int, i64, Data::Int(value) => *value);
getter!(get_float, f64, Data::Float(value) => *value);

unsafe extern "C" fn get_bool(api: *const P7CallApi, handle: P7Value, output: *mut u8) -> P7Status {
    catch_status(|| {
        if output.is_null() {
            return P7Status::InvalidArgument;
        }
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        match bridge.get(handle) {
            Some(Data::Int(value @ (0 | 1))) => {
                // SAFETY: output was checked for null.
                unsafe { *output = *value as u8 };
                P7Status::Ok
            }
            Some(_) => P7Status::TypeMismatch,
            None => P7Status::InvalidArgument,
        }
    })
}

unsafe extern "C" fn copy_string(
    api: *const P7CallApi,
    handle: P7Value,
    output: *mut u8,
    capacity: usize,
    length: *mut usize,
) -> P7Status {
    catch_status(|| {
        if length.is_null() {
            P7Status::InvalidArgument
        } else {
            let Some(bridge) = (unsafe { bridge(api) }) else {
                return P7Status::InvalidArgument;
            };
            let Some(Data::String(value)) = bridge.get(handle) else {
                return P7Status::TypeMismatch;
            };
            let bytes = value.as_bytes();
            // SAFETY: length was checked for null.
            unsafe { *length = bytes.len() };
            if output.is_null() {
                return if capacity == 0 {
                    P7Status::Ok
                } else {
                    P7Status::InvalidArgument
                };
            }
            if capacity < bytes.len() {
                return P7Status::InvalidArgument;
            }
            // SAFETY: The caller promises capacity writable bytes.
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
            P7Status::Ok
        }
    })
}

macro_rules! maker {
    ($name:ident, $ty:ty, $convert:expr) => {
        unsafe extern "C" fn $name(
            api: *const P7CallApi,
            value: $ty,
            output: *mut P7Value,
        ) -> P7Status {
            catch_status(|| {
                if output.is_null() {
                    return P7Status::InvalidArgument;
                }
                let Some(bridge) = (unsafe { bridge(api) }) else {
                    return P7Status::InvalidArgument;
                };
                let handle = bridge.push($convert(value));
                // SAFETY: output was checked for null.
                unsafe { *output = handle };
                P7Status::Ok
            })
        }
    };
}

maker!(make_int, i64, Data::Int);
maker!(make_float, f64, Data::Float);

unsafe extern "C" fn make_bool(api: *const P7CallApi, value: u8, output: *mut P7Value) -> P7Status {
    catch_status(|| {
        if value > 1 {
            return P7Status::InvalidArgument;
        }
        // SAFETY: Forwarding the same validated API and output pointers.
        unsafe { make_int(api, value as i64, output) }
    })
}

unsafe extern "C" fn make_string(
    api: *const P7CallApi,
    bytes: *const u8,
    length: usize,
    output: *mut P7Value,
) -> P7Status {
    catch_status(|| {
        if output.is_null() || (bytes.is_null() && length != 0) {
            return P7Status::InvalidArgument;
        }
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        let bytes = if length == 0 {
            &[][..]
        } else {
            // SAFETY: The caller promises length readable bytes.
            unsafe { slice::from_raw_parts(bytes, length) }
        };
        let Ok(value) = std::str::from_utf8(bytes) else {
            return P7Status::InvalidArgument;
        };
        let handle = bridge.push(Data::string(value));
        // SAFETY: output was checked for null.
        unsafe { *output = handle };
        P7Status::Ok
    })
}

#[derive(Clone, Copy)]
enum ForeignKind {
    Owned,
    Ref,
    Handle,
}

unsafe fn make_foreign(
    api: *const P7CallApi,
    type_tag: *const u8,
    type_tag_len: usize,
    host_handle: i64,
    output: *mut P7Value,
    kind: ForeignKind,
) -> P7Status {
    if output.is_null() {
        return P7Status::InvalidArgument;
    }
    let Some(bridge) = (unsafe { bridge(api) }) else {
        return P7Status::InvalidArgument;
    };
    let Some(type_tag) = bytes_string(type_tag, type_tag_len) else {
        return P7Status::InvalidArgument;
    };
    let value = match kind {
        ForeignKind::Owned => bridge.context().alloc_foreign(type_tag, host_handle),
        ForeignKind::Ref => bridge.context().alloc_foreign_ref(type_tag, host_handle),
        ForeignKind::Handle => bridge.context().alloc_foreign_handle(type_tag, host_handle),
    };
    match value {
        Ok(value) => {
            let handle = bridge.push(value);
            // SAFETY: output was checked for null.
            unsafe { *output = handle };
            P7Status::Ok
        }
        Err(RuntimeError::StaleForeignHandle { .. }) => P7Status::StaleHandle,
        Err(error) => {
            bridge.error = Some(error.to_string());
            P7Status::Error
        }
    }
}

macro_rules! foreign_maker {
    ($name:ident, $kind:expr) => {
        unsafe extern "C" fn $name(
            api: *const P7CallApi,
            type_tag: *const u8,
            type_tag_len: usize,
            host_handle: i64,
            output: *mut P7Value,
        ) -> P7Status {
            catch_status(|| {
                // SAFETY: Forwarding the caller-provided ABI pointers.
                unsafe { make_foreign(api, type_tag, type_tag_len, host_handle, output, $kind) }
            })
        }
    };
}

foreign_maker!(make_foreign_owned, ForeignKind::Owned);
foreign_maker!(make_foreign_ref, ForeignKind::Ref);
foreign_maker!(make_foreign_handle, ForeignKind::Handle);

unsafe extern "C" fn invalidate_foreign_handle(
    api: *const P7CallApi,
    type_tag: *const u8,
    type_tag_len: usize,
    host_handle: i64,
) -> P7Status {
    catch_status(|| {
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        let Some(type_tag) = bytes_string(type_tag, type_tag_len) else {
            return P7Status::InvalidArgument;
        };
        match bridge
            .context()
            .invalidate_foreign_handle(type_tag, host_handle)
        {
            Ok(()) => P7Status::Ok,
            Err(RuntimeError::StaleForeignHandle { .. }) => P7Status::StaleHandle,
            Err(error) => {
                bridge.error = Some(error.to_string());
                P7Status::Error
            }
        }
    })
}

unsafe extern "C" fn invoke_callback(
    api: *const P7CallApi,
    callback: P7Value,
    args: *const P7Value,
    arg_count: usize,
    output: *mut P7Value,
) -> P7Status {
    catch_status(|| {
        if output.is_null() || (args.is_null() && arg_count != 0) {
            return P7Status::InvalidArgument;
        }
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        let Some(closure) = bridge.get(callback).cloned() else {
            return P7Status::InvalidArgument;
        };
        if !matches!(closure, Data::Closure { .. }) {
            return P7Status::TypeMismatch;
        }
        let handles = if arg_count == 0 {
            &[][..]
        } else {
            // SAFETY: The caller promises arg_count readable handles.
            unsafe { slice::from_raw_parts(args, arg_count) }
        };
        let Some(values) = handles
            .iter()
            .map(|handle| bridge.get(*handle).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            return P7Status::InvalidArgument;
        };
        match bridge.context().call_closure(&closure, values) {
            Ok(value) => {
                let handle = bridge.push(value);
                // SAFETY: output was checked for null.
                unsafe { *output = handle };
                P7Status::Ok
            }
            Err(error) => {
                bridge.error = Some(error.to_string());
                P7Status::Error
            }
        }
    })
}

unsafe extern "C" fn set_error(
    api: *const P7CallApi,
    message: *const u8,
    length: usize,
) -> P7Status {
    catch_status(|| {
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        let Some(message) = bytes_string(message, length) else {
            return P7Status::InvalidArgument;
        };
        bridge.error = Some(message.to_string());
        P7Status::Ok
    })
}

unsafe extern "C" fn get_foreign(
    api: *const P7CallApi,
    value: P7Value,
    type_tag: *const u8,
    type_tag_len: usize,
    output: *mut i64,
) -> P7Status {
    catch_status(|| {
        if output.is_null() {
            return P7Status::InvalidArgument;
        }
        let Some(bridge) = (unsafe { bridge(api) }) else {
            return P7Status::InvalidArgument;
        };
        let Some(value) = bridge.get(value).cloned() else {
            return P7Status::InvalidArgument;
        };
        let Some(type_tag) = bytes_string(type_tag, type_tag_len) else {
            return P7Status::InvalidArgument;
        };
        match bridge.context().foreign_handle(&value, type_tag) {
            Ok(handle) => {
                // SAFETY: output was checked for null.
                unsafe { *output = handle };
                P7Status::Ok
            }
            Err(RuntimeError::StaleForeignHandle { .. }) => P7Status::StaleHandle,
            Err(error) => {
                bridge.error = Some(error.to_string());
                P7Status::TypeMismatch
            }
        }
    })
}

fn status_result(status: P7Status, operation: &str) -> Result<(), RuntimeError> {
    if status == P7Status::Ok {
        Ok(())
    } else {
        Err(RuntimeError::Other(format!(
            "{operation} failed with status {status:?}"
        )))
    }
}

fn c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: The ABI requires a readable NUL-terminated UTF-8 string.
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .map(str::to_string)
}

fn bytes_string<'a>(value: *const u8, length: usize) -> Option<&'a str> {
    if value.is_null() && length != 0 {
        return None;
    }
    let bytes = if length == 0 {
        &[][..]
    } else {
        // SAFETY: The ABI caller promises length readable bytes.
        unsafe { slice::from_raw_parts(value, length) }
    };
    std::str::from_utf8(bytes).ok()
}
