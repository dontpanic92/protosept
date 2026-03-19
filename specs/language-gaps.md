# Protosept Language Gaps

Tracked findings from building NaviText and other real-world usage.
Items are removed once fixed; new items added as discovered.

Last updated: 2026-03-19

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

### 8. ~~No enum payload destructuring in `match`~~
**RESOLVED** — see Resolved Issues table below.

### 9. No multiple return values or tuple returns
**Impact: HIGH** — Functions can only return a single value. The editor uses
`row * 100000 + col` to encode two ints into one return, then manually
decodes at every call site. Before the struct refactor, `exec_menu_action`
returned `"cr|cc|sa|d|cb"` as a pipe-delimited string with a hand-written
int parser to decode it.

Structs are the current workaround, but tuple returns would eliminate the
need for single-use structs:
```p7
// Wanted:
fn delete_selection(...) -> (int, int) { return (row, col); }
let (r, c) = delete_selection(...);

// Current: encode into int, decode at call site
fn delete_selection(...) -> int { return row * 100000 + col; }
let encoded = delete_selection(...);
let r = encoded / 100000;
let c = encoded - (r * 100000);
```
The spec defines tuple types (`(int, int)`) in §4.4, but destructuring
`let` bindings are not implemented. This is the single biggest source
of code complexity in the editor.

### 10. ~~No `bool` in practice — everything is `int` flags~~
**RESOLVED** — see Resolved Issues table below.

### 11. ~~No `&&` / `||` working with `int` flags (no compound conditions)~~
**RESOLVED** — see Resolved Issues table below.

### 12. ~~Functions cannot accept or return cross-module structs cleanly~~
**RESOLVED** — see Resolved Issues table below.

### 13. ~~No `clamp` / `min` / `max` builtins for int~~
**RESOLVED** — see Resolved Issues table below.

### 14. ~~No `array.index_of()` for finding elements~~
**RESOLVED** — see Resolved Issues table below.

### 15. ~~`else if <bare_bool>` fails — requires explicit `== true`~~
**NOT A BUG** — Investigated during git view development. The compile error was
actually caused by `tui.set_bold()` missing its required `on: int` argument
(see #16). The confusing error message made it appear that `else if bool_var`
was the problem. After fixing the missing argument, `else if bool_var {` works
correctly. Bare booleans work in both `if` and `else if` contexts.

### 16. ~~Confusing error message for missing function arguments~~
**RESOLVED** — see Resolved Issues table below.

### 17. ~~No `string.join()` method on arrays~~
**RESOLVED** — see Resolved Issues table below.

### 18. ~~No `string.trim()` / `string.trim_end()` methods~~
**RESOLVED** — see Resolved Issues table below.

### 19. No HashMap / Dictionary type
**Impact: MEDIUM** — P7 v1 has no associative container. The git module uses
parallel arrays (`items_text`, `items_type`, `items_path`) as a workaround
for what would naturally be `array<GitItem>` or `Map<string, GitItem>`.

While parallel arrays work, a builtin `Map<K, V>` would enable cleaner data
modeling for key-value lookups (e.g., tracking expanded directories by path,
caching file status by name).

### 20. No default parameter values
**Impact: LOW** — Functions like `tui.set_bold(on: int)` could benefit from
a default: `tui.set_bold(on: int = 1)`. Currently every call must provide
all arguments explicitly. The spec intentionally excludes this for
auditability, but it leads to verbose call sites for toggle-style functions.

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
| 12 | Cross-module struct passing | Fixed: map return types through `map_type_from_module`; deduplicate imported types by qualified_name in `import_type_from_module` |
| 10 | No `bool` in practice | Fixed: added `true`/`false` keywords to lexer, `BooleanLiteral` parsing; `!` prefix operator |
| 11 | No `&&` / `||` operators | Fixed: added `&&`/`||` two-character tokens to lexer; parser/codegen already supported `And`/`Or` |
| 13 | No `min`/`max`/`clamp` | Fixed: added as builtin intrinsic functions |
| 14 | No `array.index_of()` | Fixed: added as builtin intrinsic method on `array<T>` |
| 8 | No enum payload destructuring | Fixed: `match r { Result.Ok(n) => n }` pattern matching with payload binding |
| 15 | `else if <bare_bool>` fails | Not a bug: caused by confusing missing-arg error (#16). `else if bool_var` works correctly. |
| 16 | Confusing missing-arg error message | Fixed: new `MissingArgument` error variant → `Missing required argument 'x' in call to 'foo'` |
| 17 | No `string.join()` | Fixed: added `array<string>.join(sep)` as builtin intrinsic |
| 18 | No `string.trim()` methods | Fixed: added `string.trim()`, `string.trim_start()`, `string.trim_end()` as builtin intrinsics |
