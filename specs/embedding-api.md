# Protosept Embedding API

Status: initial implementation

The embedding API provides a stable layer above the interpreter's internal
stack-oriented `Context` API.

## Script calls

`embedding::Runtime` owns one interpreter context and loaded module graph.

```rust
match runtime.call("main", arguments)? {
    CallOutcome::Returned(value) => { /* normal return */ }
    CallOutcome::Threw(error) => { /* uncaught Protosept throw */ }
    CallOutcome::Trapped(trap) => { /* runtime failure */ }
}
```

Function lookup and arity validation happen before execution and return errors
instead of panicking. A trapped call restores the interpreter frame depth, so
the runtime remains usable for subsequent calls.

`Returned(None)` currently represents a unit return because the interpreter
does not have a dedicated runtime `unit` value.

## Typed native functions

Native functions are registered with a monomorphic `NativeSignature`:

```rust
runtime.register_native_function(
    "host.add",
    NativeSignature::new(
        vec![NativeType::Int, NativeType::Int],
        Some(NativeType::Int),
    ),
    |_context, args| {
        // Return Some(value), or None for unit.
        Ok(Some(Data::Int(42)))
    },
);
```

The adapter:

- Captures extension state through an ordinary Rust closure.
- Removes exactly the declared number of arguments from the VM stack.
- Preserves source argument order.
- Validates argument and return runtime shapes.
- Converts mismatches into runtime traps.

The older `register_host_function` API remains available for builtins and
foreign-proto compatibility, but now accepts stateful closures. New extensions
should use typed native functions.

## Rooted values

`Runtime::root` creates a runtime-scoped `RootedValue`. Clones share one root,
and dropping the final clone schedules removal from the runtime's GC roots.

A root:

- Keeps recursively referenced values alive.
- Is rejected when used with a different runtime.
- Can be resolved only while its runtime remains alive.

Root release is flushed before and after script calls and before mutable
context access.

## Callbacks

`Runtime::root_callback` validates and roots a closure as a `CallbackHandle`.
Native functions can capture this handle and invoke it through their supplied
`&mut Context`, including re-entrant invocation while an outer script call is
suspended.

Callback invocation reports the same returned/threw/trapped outcome categories
as top-level script calls.

## Runtime shutdown

Embedders that load dynamic native extensions must call
`Runtime::shutdown()` before discarding the runtime. Shutdown is idempotent and
invokes optional extension hooks in reverse load order while the interpreter
context remains alive. After each successful hook, the runtime removes that
extension's registered callbacks and userdata before unloading its library.

A shutdown error identifies the extension and intentionally keeps the unsafe
library and its host context alive. Calls, module loads, roots, and extension
loads are rejected after shutdown starts. `Drop` attempts the same sequence as
a safety fallback, but explicit shutdown is required when the caller needs to
report an error.
