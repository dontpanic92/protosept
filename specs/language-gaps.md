# Protosept Language Gaps — Findings from NaviText Implementation

## Problem
While building a TUI text editor in Protosept, several language/runtime gaps
were encountered. This document catalogs them for review before implementing fixes.

## Findings

### 1. No forward function references (compiler limitation)
**Impact: HIGH** — Functions must be defined before use. `main()` must be the
last function in the file. Mutual recursion is impossible. This forced careful
manual ordering of ~15 functions and made the code harder to organize.

**Expected:** Functions in the same module should be callable regardless of
declaration order (two-pass compilation).

### 2. No bare `return;` in unit functions
**Impact: MEDIUM** — `return;` (without a value) is a parse error. Early returns
from void functions require restructuring into flag-based patterns:
```p7
// Wanted:
fn toggle(path: string, list: box<array<string>>) {
    // ...find and remove...
    return;  // ERROR: Unexpected token: Semicolon
}

// Had to write:
fn toggle(path: string, list: box<array<string>>) {
    var found_idx = 0 - 1;
    // ...search...
    if found_idx >= 0 { remove(...); } else { add(...); }
}
```

### 3. Null comparison broken for non-primitive nullable types
**Impact: MEDIUM** — `if content != null` fails with "Type mismatch: ?string != ?unit"
when `content: ?string`. Works for `?int`. Had to use `??` workaround:
```p7
// Wanted:
if content != null { let text = content!; ... }

// Had to write:
let text = content ?? "";
```

### 4. Box dereference rejected for non-primitive types
**Impact: MEDIUM** — `*box<array<T>>` fails at compile time ("only primitive types
supported"). Inconsistent: `boxed.len()` and `boxed[i]` work (auto-borrow handles it),
but explicit `*boxed` doesn't. This blocks patterns like `(*boxed)[i]`.

**Workaround:** `boxed[i]` works directly (the compiler has a special path for
indexing on boxed arrays).

### 5. No `else if` chains (or unclear support)
**Impact: MEDIUM** — Key handling required deeply nested if/else or flat sequential
if statements. Neither is clean:
```p7
// Sequential ifs (all evaluated even after match):
if key == 7 { ... }
if key == 8 { ... }
if key == 1 { ... }

// Nested else (pyramid of doom):
if key == 7 { ... } else { if key == 8 { ... } else { if key == 1 { ... } } }
```

### 6. No closures/lambdas (known v1 limitation)
**Impact: MEDIUM** — Forced a stateful polling model for TUI events instead of
callbacks. Required thread-local storage on the Rust side.

### 7. Verbose string concatenation
**Impact: MEDIUM** — No `+` operator for strings. Building strings requires
chained `.concat()` calls:
```p7
// Repeatedly building strings:
var text = "";
text = text.concat("  ");
text = text.concat(name);
text = text.concat(" ");
// vs: text = text + "  " + name + " "
```
String interpolation (`f"..."`) helps but can't be used for incremental building
in loops.

### 8. No array insert/remove at index
**Impact: MEDIUM** — Had to implement `insert_line` and `remove_line` manually by
copying to a temp array. ~25 lines of boilerplate each. These should be builtin
array methods on `box<array<T>>`.

### 9. No `for` loop with index
**Impact: LOW-MEDIUM** — Every array iteration required manual `var i = 0; while i < count { ...; i = i + 1; }` (5 lines of overhead per loop). An indexed for
or `for i, elem in arr` would eliminate most `while` loops.

### 10. Negative int literals
**Impact: LOW** — Unclear if `-1` works as a literal. Had to write `0 - 1` to be safe.
If the parser supports unary minus on literals, this is just a documentation issue.

### 11. No `match` on integers in practice
**Impact: LOW** — The spec supports integer match patterns, but key code dispatch
would benefit from it. Used sequential `if` chains instead (unclear if match on
int works reliably in current compiler).

### 12. `int` is i32 at runtime but spec says i64
**Impact: LOW** — Spec says `int` is i64, but `Data::Int(i32)` in the runtime.
Not a problem for the editor, but could surprise users with overflow on larger values.

### 13. No type aliases
**Impact: LOW** — `box<array<string>>` appears 30+ times in function signatures.
A `type Lines = box<array<string>>` would improve readability.

### 14. Single-file organization pressure
**Impact: LOW** — The combination of no forward references + uncertain cross-module
struct support pushed all editor logic into one ~730-line file. Multi-file
organization felt risky.
