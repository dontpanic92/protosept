#ifndef PROTOSEPT_EXTENSION_H
#define PROTOSEPT_EXTENSION_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define P7_NATIVE_ABI_VERSION 1u
#define P7_EXTENSION_INIT_SYMBOL "p7_extension_init_v1"

typedef enum P7Status {
    P7_STATUS_OK = 0,
    P7_STATUS_ERROR = 1,
    P7_STATUS_INVALID_ARGUMENT = 2,
    P7_STATUS_TYPE_MISMATCH = 3,
    P7_STATUS_STALE_HANDLE = 4,
    P7_STATUS_PANIC = 5
} P7Status;

typedef enum P7NativeType {
    P7_TYPE_ANY = 0,
    P7_TYPE_INT = 1,
    P7_TYPE_FLOAT = 2,
    P7_TYPE_BOOL = 3,
    P7_TYPE_STRING = 4,
    P7_TYPE_ARRAY = 5,
    P7_TYPE_TUPLE = 6,
    P7_TYPE_MAP = 7,
    P7_TYPE_CLOSURE = 8,
    P7_TYPE_FOREIGN = 9
} P7NativeType;

typedef enum P7ValueKind {
    P7_VALUE_INVALID = 0,
    P7_VALUE_INT = 1,
    P7_VALUE_FLOAT = 2,
    P7_VALUE_STRING = 3,
    P7_VALUE_ARRAY = 4,
    P7_VALUE_TUPLE = 5,
    P7_VALUE_MAP = 6,
    P7_VALUE_CLOSURE = 7,
    P7_VALUE_FOREIGN = 8,
    P7_VALUE_NULL = 9,
    P7_VALUE_OTHER = 10
} P7ValueKind;

typedef struct P7Value {
    uint64_t token;
} P7Value;

typedef struct P7CallApi P7CallApi;
typedef struct P7HostApi P7HostApi;

typedef P7Status (*P7NativeCallback)(
    void *userdata,
    const P7CallApi *api,
    const P7Value *args,
    size_t arg_count,
    P7Value *output);

typedef void (*P7DropUserdata)(void *userdata);

typedef struct P7NativeFunctionDescriptor {
    size_t struct_size;
    const char *name;
    const P7NativeType *params;
    size_t param_count;
    P7NativeType result;
    uint8_t has_result;
    P7NativeCallback callback;
    void *userdata;
    P7DropUserdata drop_userdata;
} P7NativeFunctionDescriptor;

struct P7HostApi {
    uint32_t abi_version;
    size_t struct_size;
    void *runtime;
    P7Status (*register_function)(
        void *runtime,
        const P7NativeFunctionDescriptor *descriptor);
    P7Status (*register_foreign_type)(
        void *runtime,
        const char *type_tag,
        const char *finalizer);
    P7Status (*invalidate_foreign_handle)(
        void *runtime,
        const uint8_t *type_tag,
        size_t type_tag_len,
        int64_t host_handle);
    P7Status (*invoke_rooted_callback)(void *runtime, uint64_t callback_token);
    P7Status (*release_rooted_callback)(void *runtime, uint64_t callback_token);
};

struct P7CallApi {
    uint32_t abi_version;
    size_t struct_size;
    void *context;
    P7ValueKind (*value_kind)(const P7CallApi *, P7Value);
    P7Status (*get_int)(const P7CallApi *, P7Value, int64_t *);
    P7Status (*get_float)(const P7CallApi *, P7Value, double *);
    P7Status (*get_bool)(const P7CallApi *, P7Value, uint8_t *);
    P7Status (*copy_string)(
        const P7CallApi *, P7Value, uint8_t *, size_t, size_t *);
    P7Status (*make_int)(const P7CallApi *, int64_t, P7Value *);
    P7Status (*make_float)(const P7CallApi *, double, P7Value *);
    P7Status (*make_bool)(const P7CallApi *, uint8_t, P7Value *);
    P7Status (*make_string)(
        const P7CallApi *, const uint8_t *, size_t, P7Value *);
    P7Status (*make_foreign_owned)(
        const P7CallApi *, const uint8_t *, size_t, int64_t, P7Value *);
    P7Status (*make_foreign_ref)(
        const P7CallApi *, const uint8_t *, size_t, int64_t, P7Value *);
    P7Status (*make_foreign_handle)(
        const P7CallApi *, const uint8_t *, size_t, int64_t, P7Value *);
    P7Status (*invalidate_foreign_handle)(
        const P7CallApi *, const uint8_t *, size_t, int64_t);
    P7Status (*invoke_callback)(
        const P7CallApi *,
        P7Value,
        const P7Value *,
        size_t,
        P7Value *);
    P7Status (*set_error)(const P7CallApi *, const uint8_t *, size_t);
    P7Status (*get_foreign)(
        const P7CallApi *,
        P7Value,
        const uint8_t *,
        size_t,
        int64_t *);
    P7Status (*retain_callback)(
        const P7CallApi *,
        P7Value,
        uint64_t *);
    void *runtime;
};

typedef P7Status (*P7ExtensionInit)(const P7HostApi *api);

#ifdef __cplusplus
}
#endif

#endif
