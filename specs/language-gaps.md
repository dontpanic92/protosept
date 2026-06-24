# Protosept Language Gaps

Tracked findings from building NaviText and other real-world usage.
Items are removed once fixed; new items added as discovered.

Last updated: 2026-06-24

---

## Resolved

### Mutability model: `ref<T>` / `refmut<T>` + value-type interior mutation
**Status: shipped.** Borrows are now two-strength: `ref<T>` is a **read-only** view and
`refmut<T>` is a **mutable** view (receivers `ref self` / `refmut self`). Writing through
a `ref<T>` is rejected. Interior mutation of a **value** struct or array no longer
requires `box`: it is gated on the place being mutable (a `let mut` binding, a `box`, or
a `refmut`). `box<T>` now means identity/sharing/escape only. `let mut` **local** slots
are addressable (`ref`/`refmut`); `let mut` module-level bindings remain non-addressable.
Value arrays support `xs[i] = v`, `push`, `pop`, `insert`, `remove`, `clear` on a
`let mut` local (copy-on-write with store-back); nested/handle-rooted value arrays still
use `box<array<T>>`. Regression coverage: `p7/tests/ref_mutability.rs`,
`p7/tests/value_array_mutation.rs`, `tests/test_ref_mut_self.p7`,
`tests/test_proto_ref_mut_self.p7`.

---

## Open Issues

### 1. Box dereference rejected for non-primitive types
**Impact: MEDIUM** — `*box<array<T>>` fails at compile time ("only primitive types
supported"). Inconsistent: `boxed.len()` and `boxed[i]` work (auto-borrow handles it),
but explicit `*boxed` doesn't. This blocks patterns like `(*boxed)[i]`.

**Workaround:** `boxed[i]` works directly (compiler special-cases indexing on
boxed arrays). Method calls also work via auto-borrow.

### 2. No `for` loop with index — RESOLVED (array form)
**Status: shipped (`for x in arr`, `for i, x in arr`).** See the “Iteration”
section of `protosept-language.md`. The element binding is by value when the
element type is Copy-treated and `ref<T>` otherwise (matching the existing
`let t = ref(self.tabs[i]);` idiom). Length is snapshotted at loop entry, so
mid-loop pushes are not visited. Iterating over a `box<...>`/`ref<...>` array
unwraps one layer automatically.

Range form (`for i in 0..n`) is still open — see item #6.

### 3. No type aliases
**Impact: LOW** — `box<array<string>>` / `refmut<array<string>>` appear repeatedly in
function signatures. A `type Lines = box<array<string>>` would improve readability
significantly.

### 4. No `match` on integers reliably — RESOLVED
**Impact: LOW** — Spec §9.6 `match` on int / bool / enum is now fully
supported and verified end-to-end:

- Trailing comma after the last arm is optional.
- `true` / `false` literal patterns parse and run.
- Bare identifier patterns (`n => ...`) bind to the scrutinee (irrefutable).
- Non-exhaustive matches are rejected at compile time with a clear
  `Non-exhaustive match on <T>: ...` error.
- Or-patterns `p1 | p2 | ...` are supported for literal / unit-variant
  alternatives in `match` and `try ... else` arms (v1: no bindings or
  destructuring inside an alternative); they participate in exhaustiveness
  (e.g. `true | false` for `bool`, all-variants for enums).

Regression coverage lives in
`radiance/protosept/p7/tests/match_int_enum.rs`.

### 5. No `+=`, `-=`, `*=` compound assignment
**Impact: MEDIUM** — Mutable variable updates require repeating the variable name:
```p7
cursor_col = cursor_col + 1;  // instead of cursor_col += 1;
```
Very common pattern in the editor event loop.

### 6. No `for` loop over ranges — RESOLVED
**Status: shipped.** `builtin.Range(start, end)` (half-open) and
`builtin.RangeIncl(start, end)` (closed) are first-class iterables in
the builtin package; both conform to `Iterable` and produce a fresh
`Iterator` from `.iter()`. `for i in builtin.Range(0, n) { ... }` is the
canonical counting loop. The `builtin` module symbol is auto-imported,
so no explicit `import builtin;` line is needed.

### 7. No `int.to_string()` / `int.display()` method
**Impact: LOW** — Cannot convert int to string except via string interpolation
`f"{n}"`. A direct `n.to_string()` or `n.display()` method would be useful
for building strings programmatically.

### 20. No default parameter values — RESOLVED
**Status: shipped.** Default values are supported on **both function
parameters** (`fn f(on: int = 1)`) **and record-struct fields**
(`struct Text(content: string, color: int = WHITE, size: float = 12.0)`).
Omitted trailing arguments — positionally (`Text("hi")`) or by name
(`Text(content = "hi")`) — fall back to the default expression, which may
reference module-level constants. Regression coverage:
`radiance/protosept/p7/tests/default_values.rs`.

### 21. No implicit auto-boxing of bare values to `box<P>` — RESOLVED
**Status: shipped.** At checking-context sites (array-literal elements,
annotated `let`, argument passing, returns) a bare struct/enum *value* whose
type declares conformance to proto `P` now auto-boxes to `box<P>`. This lets
declarative widget children be written `children = [Text(...), Button(...)]`
for an `array<box<Element>>` parameter without an explicit `box(...)` per
element. Unlike the spec's reinterpreting `box<T> -> box<P>` coercion (§18.5),
this allocates a fresh box for the temporary. A non-conforming bare value at a
`box<P>` site is now a compile-time type error (previously a silent
miscompile). Regression coverage:
`radiance/protosept/p7/tests/array_autobox_proto.rs`.

