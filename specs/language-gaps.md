# Protosept Language Gaps

Tracked findings from building NaviText and other real-world usage.
Items are removed once fixed; new items added as discovered.

Last updated: 2026-03-17

---

## Open Issues

### 1. Box dereference rejected for non-primitive types
**Impact: MEDIUM** — `*box<array<T>>` fails at compile time ("only primitive types
supported"). Inconsistent: `boxed.len()` and `boxed[i]` work (auto-borrow handles it),
but explicit `*boxed` doesn't. This blocks patterns like `(*boxed)[i]`.

**Workaround:** `boxed[i]` works directly (compiler special-cases indexing on
boxed arrays). Method calls also work via auto-borrow.

### 2. No `for` loop with index
**Impact: MEDIUM** — Every array iteration requires:
```p7
var i = 0;
while i < count {
    let item = arr[i];
    // ...
    i = i + 1;
}
```
An indexed `for` (e.g., `for item in arr { ... }` or `for i, item in arr { ... }`)
would eliminate most `while` loops and the manual index boilerplate.

### 3. No type aliases
**Impact: LOW** — `box<array<string>>` appears 30+ times in function signatures.
A `type Lines = box<array<string>>` would improve readability significantly.

### 4. No `match` on integers reliably
**Impact: LOW** — The spec supports integer match patterns, but key code dispatch
in the editor uses sequential `if` chains. Unclear if `match` on `int` works
reliably in the current compiler for all cases.

### 5. No `+=`, `-=`, `*=` compound assignment
**Impact: MEDIUM** — Mutable variable updates require repeating the variable name:
```p7
cursor_col = cursor_col + 1;  // instead of cursor_col += 1;
```
Very common pattern in the editor event loop.

### 6. No `for` loop over ranges
**Impact: MEDIUM** — Counting loops require manual `while` with a counter variable.
A `for i in 0..n { ... }` range syntax would be cleaner and less error-prone.

### 7. No `int.to_string()` / `int.display()` method
**Impact: LOW** — Cannot convert int to string except via string interpolation
`f"{n}"`. A direct `n.to_string()` or `n.display()` method would be useful
for building strings programmatically.

### 8. No enum payload destructuring in `match`
**Impact: MEDIUM** — Cannot extract payload values from enum variants in match arms:
```p7
// Wanted:
match result {
    Result.Ok(value) => value,
    Result.Err(msg) => default,
}
// Current: must use wildcard binding, no payload access
```

---

## Resolved Issues

| # | Issue | Resolution |
|---|-------|-----------|
| 1 | No forward function references | Fixed: two-pass compilation |
| 2 | No bare `return;` | Fixed: parser accepts `return;` |
| 3 | Null comparison for `?string` | Fixed: `==`/`!=` works between any nullable types |
| 5 | No `else if` chains | Was already working (parser recursion) |
| 6 | No closures/lambdas | Fixed: full closure support with captures |
| 7 | Verbose string concatenation | Fixed: `+` operator for strings |
| 8 | No array insert/remove | Fixed: `insert()` and `remove()` builtins |
| 10 | Negative int literals | Was already working |
| 12 | `int` is i32 not i64 | Fixed: changed to i64 throughout |
| 14 | Single-file organization | Fixed: cross-module builtin resolution |
| 9* | `string.contains()` | Fixed: added as builtin intrinsic |
| 10* | Implicit return | Was already working (parser `BlockValue`) |
