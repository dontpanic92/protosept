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

### 21. No cross-module struct destructuring in `let` patterns
**Impact: MEDIUM** — The pattern `let types.Pos(r, c) = some_call()` fails
with "Type not found: types". The parser only accepts a bare identifier for
struct patterns (e.g., `let Pos(r, c) = ...`), not a module-qualified path.

This means structs defined in a shared `types.p7` module cannot be
destructured in consuming modules. The workaround is to assign to a
temporary and use field access:
```p7
let p = some_call();
let r = p.row;
let c = p.col;
```

This is a significant gap because moving shared types to a common module
(which is good architecture) breaks destructuring patterns. Either the
parser needs to accept qualified names in struct patterns, or the type
resolver needs to look up unqualified names through imports.

### 22. Import name shadows local variables (no namespace separation)
**Impact: HIGH** — When a module is imported with a short name that matches
a local variable, the local variable shadows the module name, silently
breaking all module function calls:
```p7
import navitext.tabs;
let tabs = box([...]);  // shadows the module
tabs.clear();           // ERROR: calls module function, not array method
```
The workaround is `import navitext.tabs as tab_ops;` to avoid the collision,
but this is a footgun. The compiler should either:
- Error when a local variable shadows an imported module name, or
- Use a separate namespace for module lookups vs variable lookups, or
- Prefer the variable for method-style calls and the module for `module.fn()` calls

### 23. No struct update / spread syntax
**Impact: HIGH** — The `Tab` struct has 16 positional fields. To update a
single field (e.g., `dirty`), the entire struct must be reconstructed with
all fields explicitly listed:
```p7
let old = tabs[idx];
tabs.set(idx, types.Tab(
    old.path, old.title, old.lines, old.lang,
    dirty, old.read_only, old.is_diff,           // <-- one field changed
    old.cursor_row, old.cursor_col, old.scroll_row, old.scroll_col,
    old.anchor_row, old.anchor_col, old.sel_active, old.sel_selecting,
    old.max_line_len,
));
```
A struct update syntax like `Tab { ...old, dirty: new_value }` would
eliminate this boilerplate. Without it, large structs become very
error-prone to update — forgetting or misordering a field is a common
mistake, and the code is unreadable.

This was the single largest source of code duplication in the NaviText
refactoring. Every tab save/load operation repeats all 16 fields.

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

### 25. Box parameter move tracking may be overly strict
**Impact: HIGH** — In the p7 type system, `box<T>` is documented as
copy-treated (handle semantics), and `is_copy_treated()` returns `true`
for `BoxType`. However, during the NaviText refactoring, passing a
`box<array<T>>` function parameter to another function and then reusing
the parameter triggered "Use of moved value":
```p7
pub fn open_diff(tabs: box<array<Tab>>, ...) {
    save_tab(tabs, ...);  // passes tabs to another function
    tabs.push(new_tab);   // ERROR: Use of moved value: tabs
}
```
This contradicts the copy-treated semantics. Method calls on the same
box (`.len()`, `.set()`, `[idx]`) work fine, but passing to a function
marks the parameter as moved even though `box` should be a copyable
handle.

The workaround is to inline all helper function calls — which defeats
the purpose of extracting reusable functions. This was the most impactful
limitation during the refactoring: it forced every tab lifecycle function
to inline ~10 lines of save/load logic rather than calling shared helpers.

If box is truly copy-treated, the move checker should not flag re-use
after passing to a function. If box is intentionally move-semantics,
the `is_copy_treated()` implementation is wrong and `ref<box<T>>`
should be used for borrowing — but the ergonomics of that are unclear.

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
