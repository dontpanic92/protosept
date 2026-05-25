# Protosept Language Gaps

Tracked findings from building NaviText and other real-world usage.
Items are removed once fixed; new items added as discovered.

Last updated: 2026-03-21

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
**Impact: LOW** — `box<array<string>>` appears 30+ times in function signatures.
A `type Lines = box<array<string>>` would improve readability significantly.

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

### 20. No default parameter values
**Impact: LOW** — Functions like `tui.set_bold(on: int)` could benefit from
a default: `tui.set_bold(on: int = 1)`. Currently every call must provide
all arguments explicitly. The spec intentionally excludes this for
auditability, but it leads to verbose call sites for toggle-style functions.
