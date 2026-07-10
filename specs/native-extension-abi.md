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

The runtime loads dependency extensions before root-package extensions and
keeps every successful library loaded until its runtime is destroyed.

## Function registration

`P7NativeFunctionDescriptor` contains:

- A NUL-terminated UTF-8 intrinsic name.
- A monomorphic parameter and result signature.
- A C callback.
- An opaque userdata pointer and optional destructor.

The runtime takes ownership of `userdata` only when `register_function`
returns `P7_STATUS_OK`. The destructor runs once when the registration is
removed or the runtime is destroyed. Descriptor strings and signature arrays
are borrowed only for the synchronous registration call.

The callback receives transient opaque `P7Value` tokens. Tokens and the
`P7CallApi` table remain valid only until that callback returns. Input tokens
are borrowed. A token written to `output` is copied by the runtime before the
callback returns. `output.token == 0` represents a unit return.

Callbacks report a detailed failure by calling `set_error` and then returning
a non-OK status. No C++, Pascal, Rust, or other language exception may unwind
across an ABI function. Extensions must catch their own exceptions.

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

Long-lived rooted callback subscriptions are a separate event-interop layer
and are not represented by transient `P7Value` tokens.
