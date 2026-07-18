# Native Extension ABI

Status: version 1

The native extension ABI lets a dynamically loaded library register typed
functions and foreign-object metadata without depending on Rust layout or the
interpreter stack. The normative C declarations are in
`p7/include/protosept_extension.h`.

## Entry point

An extension exports:

```c
P7Status p7_extension_init_v1(const P7HostApi *api);
```

The extension must check `api->abi_version` and `api->struct_size` before using
the table. The table itself is borrowed only during initialization; extensions
may copy its function pointers and opaque `runtime` value. Registration is
transactional: if initialization returns a status
other than `P7_STATUS_OK`, all registrations made by that initializer are
removed before the library is unloaded.

The runtime loads dependency extensions before root-package extensions.
Successful libraries remain loaded for the process lifetime because GUI
toolkits and some language runtimes do not support safe dynamic unloading.
Failed initializers are still unloaded after their registrations are rolled
back.

All ABI tables are append-only. Before reading an extension field, test that
`struct_size` reaches the end of that field. The C header provides
`P7_API_HAS_FIELD(api, P7HostApi, field)` and
`P7_API_HAS_FIELD(api, P7CallApi, field)` for this purpose. An extension must
not require the current full structure size when it only needs an older
prefix, and must not read or call a field that is outside the advertised size.

## Function registration

`P7NativeFunctionDescriptor` contains:

- A NUL-terminated UTF-8 intrinsic name.
- A monomorphic parameter and result signature.
- A C callback.
- An opaque userdata pointer and optional destructor.

`P7NativeType` is append-only in ABI v1. The original values `ANY` through
`FOREIGN` remain discriminants 0 through 9. Fixed-width integer kinds are:

| Kind | Discriminant | Accepted `Data::Int` range |
|---|---:|---:|
| `I8` | 10 | `-128..=127` |
| `U8` | 11 | `0..=255` |
| `I16` | 12 | `-32768..=32767` |
| `U16` | 13 | `0..=65535` |
| `I32` | 14 | signed 32-bit |
| `U32` | 15 | unsigned 32-bit |
| `I64` | 16 | signed 64-bit |
| `U64` | 17 | `0..=INT64_MAX` (temporary runtime limitation) |

The stack adapter validates every incoming argument and outgoing callback
result against its declared fixed-width range. A mismatch traps the script
call before an invalid value crosses the typed boundary. The ABI still uses
`get_int`/`make_int` with `int64_t` as the value carrier.

No field was inserted into an ABI v1 structure for this feature; only enum
values were appended, preserving existing structure sizes and field offsets.

The runtime takes ownership of `userdata` only when `register_function`
returns `P7_STATUS_OK`. The destructor runs once when the registration is
removed or the runtime is destroyed. Descriptor strings and signature arrays
are borrowed only for the synchronous registration call.

The callback receives transient opaque `P7Value` tokens. Tokens and the
`P7CallApi` table remain valid only until that callback returns. Input tokens
are borrowed. A token written to `output` is copied by the runtime before the
callback returns. `output.token == 0` represents a unit return.

Callbacks report a failure by setting error information and then returning a
non-OK status. `set_error` remains supported and records an unstructured
native error whose message is the supplied UTF-8 text. When
`P7_API_HAS_FIELD(api, P7CallApi, set_error_details)` is true,
`set_error_details` records UTF-8 operation identifier, exception/error class,
and message fields. Each pointer may be null only when its corresponding
length is zero. Invalid UTF-8 or pointer/length combinations return
`P7_STATUS_INVALID_ARGUMENT` without replacing a previously recorded error.

Error state is scoped to one callback invocation. It is cleared before every
call and is stacked across nested or re-entrant calls. A non-OK return uses the
most recently explicitly set error; generic status mapping only applies when
the callback did not record one. Native errors that trap during callback
re-entry, including rooted callback invocation, retain their structured
fields.

No C++, Pascal, Rust, or other language exception may unwind across an ABI
function. Extensions must catch their own exceptions.

## Strings and values

String data is UTF-8 and never implicitly NUL-terminated. `copy_string` first
reports the required byte length and then copies into caller-owned storage.
`make_string` copies its input before returning.

The ABI intentionally exposes no Rust enum, string, array, map, closure, or
foreign-object representation. All access goes through `P7CallApi`.

## Foreign values

The call API distinguishes:

- `make_foreign_owned`: owning value; its registered finalizer runs once.
- `make_foreign_ref`: temporary non-owning value without invalidation.
- `make_foreign_handle`: persistent non-owning identity tracked by the
  runtime.

`get_foreign` validates a foreign value against an expected type tag, checks
its box and persistent-handle generations, and returns the opaque host token.

`invalidate_foreign_handle` invalidates every persistent value with the same
type tag and host token. It is available both during a callback and through
the runtime-level host table, allowing native destruction notifications to
invalidate script-visible handles. Later dereference traps with
`StaleForeignHandle`. If the host later reuses the same token for a new object,
the runtime assigns a new generation; old values remain stale.

The runtime retains generation state for every identity it has seen. This is
intentional: forgetting an invalidated identity could restart its generation
and accidentally make an old script value valid when a host token is reused.
Hosts with an unbounded token space should account for this per-runtime
bookkeeping.

The opaque `runtime` pointer remains valid until runtime destruction and is
thread-affine. Extensions must not call it concurrently or after teardown.

## Callback invocation

`invoke_callback` invokes a closure token during a native call and preserves
the interpreter's suspended state. The callback and argument tokens are
borrowed; the returned token is transient like every other call value.

For long-lived subscriptions, `retain_callback` converts a transient closure
value into a runtime-owned callback token. The extension copies the
runtime-level host table during initialization and later uses:

- `invoke_rooted_callback(runtime, token)` to invoke the zero-argument closure.
- `release_rooted_callback(runtime, token)` to unregister it and release its
  GC root.
- `invoke_rooted_callback_values(runtime, token, args, count, output)` to pass
  copied   integer, float, boolean, UTF-8 string, or persistent foreign-handle
  arguments and receive an integer or float result.

Tokens are monotonic within a runtime, so a released token cannot alias a
later callback. Invoking or releasing a stale token returns an error status.
The runtime pointer and callback operations are thread-affine.

`P7CallbackValue.kind` is an integer wire discriminant and is always validated;
unknown values return `P7_STATUS_TYPE_MISMATCH`. `FOREIGN` uses `int_value` for
the host handle and `bytes`/`length` for the dynamic UTF-8 type tag. Strings and
foreign values are input-only because the ABI does not expose borrowed runtime
storage for either result representation. Protosept booleans use integer
results (`0` or `1`) on this callback-result path. Mutable native event
arguments are represented by returning their replacement value.
