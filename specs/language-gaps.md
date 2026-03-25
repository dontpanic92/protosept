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

### 9. ~~No multiple return values or tuple returns~~
**RESOLVED** — see Resolved Issues table below.

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

### 21. ~~No cross-module struct destructuring in `let` patterns~~
**RESOLVED** — see Resolved Issues table below.

### 22. ~~Import name shadows local variables (no namespace separation)~~
**RESOLVED** — see Resolved Issues table below.

### 23. ~~No struct update / spread syntax~~
**RESOLVED** — see Resolved Issues table below.

### 24. No higher-order array methods (map, filter, for_each)
**Impact: MEDIUM** — While closures are supported in p7 (and captured
variables work), there are no builtin higher-order methods on arrays.
Every transformation requires manual `while` loops:
```p7
// What could be: let names = items.map((item) => item.name);
var names = box([""]);
names.clear();
var i = 0;
while i < items.len() {
    names.push(items[i].name);
    i = i + 1;
}
```
Adding `array.map()`, `array.filter()`, `array.for_each()`, and
`array.any()` would significantly reduce boilerplate in data processing
code. The git module and tree module are full of these manual patterns.

### 25. ~~Box parameter move tracking may be overly strict~~
**RESOLVED** — see Resolved Issues table below.

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
| 9 | No tuple returns / destructuring | Fixed: implemented tuple types `(T1, T2)`, literals `(a, b)`, field access `.0`/`.1`, destructuring `let (a, b) = ...`, and match patterns |
| 22 | Import name shadows local variables | Fixed: local variable methods now take priority over module calls in field access dispatch. Module functions still accessible for non-method names. |
| 25 | Box parameter move tracking overly strict | Fixed: variables and parameters had overlapping ID spaces in a shared `moved_variables` HashSet. Split into separate `moved_variables` and `moved_params` sets. |
| 21 | No cross-module struct destructuring | Fixed: codegen tries `resolve_qualified_type_name("module.Type")` when direct lookup fails in both `let` patterns and `match` arms |
| 23 | No struct update/spread syntax | Fixed: `Type(...base, field = val)` syntax — lexer (`...` token), parser, AST variant, codegen using `Ldfield` for unchanged fields + `NewStruct` |
