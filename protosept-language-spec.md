# Protosept Language Specification (Draft 1.1)

Status: Draft (v1 target)

> **Protosept** is the public name of the language.
> The short name **p7** may still appear in tooling, file extensions,
> and internal identifiers. Unless otherwise stated, "Protosept" and "p7" refer to the same language.

Design Goals (North Star)

*   **Statically Typed, Scripting Feel**: Concise abstractions and high-level ergonomics, backed by the rigor of a compiled, type-safe system.
*   **Auditability First (Human-Centric Review)**: Code is read far more often than it is written, increasingly by humans reviewing AI-generated output. Syntax prioritizes clarity of *intent*, *data flow*, and *cost* over brevity. The "canonical" form of the code must be unambiguous.
*   **Explicit Data Semantics**: The type system MUST transparently communicate ownership and cost. A reader must be able to distinguish a Value (`T`), a Borrowed View (`ref<T>`), and an Owned Heap Handle (`box<T>`) from the signature alone.
*   **Seamless Host Interop**: Designed for embedding. The explicit memory model (values vs. handles) maps predictably to host systems, allowing safe, zero-cost sharing of host objects (via `ref`) and clear ownership boundaries (via `box`).
*   **Ergonomics via Tooling**: While the stored code is explicit, the authoring experience is low-friction. The compiler supports syntactic shorthands (sigils) for rapid entry, which tooling can canonize to explicit forms for review.
*   **Correctness by Default**: Explicit nullability, explicit borrowing, and explicit identity prevent entire classes of runtime errors.

This document defines the intended *language semantics*. Where details are not finalized, sections use `[[TODO]]`.

Normative keywords:
- **MUST / MUST NOT**: mandatory requirements.
- **ERROR**: a compile-time error.
- **TRAP**: an unrecoverable runtime failure (panic); cannot be caught by `try`.
- **THROW**: a typed, recoverable error value (an enum) that *may* be handled by `try`.

---

## 0. Notation and core terms

- `T, U, V` are types.
- `x, y, z` are identifiers.
- `null` denotes the null value (only inhabits nullable types).
- **Slot**: a storage location introduced by `let`, `var`, or a parameter.
- **Addressable location** (v1): a `let`-introduced slot, a parameter slot, or a field/sub-location of an addressable base where the language provides addressability (see §7.1, §7.2, §11.4). Note: `var` slots are NOT addressable locations in v1.
- **Copy-treated**: implicit copy occurs at value-flow sites (§6.1). (Distinct from “Copy-eligible”.)


### 0.1 Contextual typing (bidirectional typing)

The type system uses **bidirectional typing** where:

- **Synthesize** (↑): the expression's type is determined from its structure and subexpression types independently of context.
  - Example: `1 + 2` synthesizes type `int`; `"hello"` synthesizes type `string`.
  
- **Check** (↓): the expression is checked against an **expected type** (also called **contextual type**) provided by the surrounding context.
  - Example: In `let x: ?int = null;`, the `null` literal is checked against the expected type `?int`.

- **Expected type / Contextual type**: a type determined by the surrounding context (e.g., explicit type annotation, function parameter type, return type) that guides type checking and enables inference for expressions that cannot synthesize a type on their own.

**Pragmatic inference rule**: When an expression requires type arguments (e.g., generic function calls, generic struct/enum construction), the compiler:
1. Attempts to infer type arguments from:
   - Argument types (for calls/construction)
   - Expected type (if available)
2. If a unique instantiation can be determined, the type arguments MAY be omitted.
3. If multiple instantiations are possible or none can be determined, it is an ERROR; the compiler MUST report the ambiguity and require explicit type arguments.

This approach maintains explicitness at API boundaries (function signatures remain fully annotated) while reducing ceremony at call sites when types are unambiguous.

---

## 1. Program structure

A program is a sequence of top-level items:

- Function declarations: `fn ...`
- Struct declarations: `struct ...`
- Enum declarations: `enum ...`
- Proto declarations: `proto ...`

Top-level executable statements are not allowed in v1. Execution begins when the host invokes an entrypoint function via embedding (e.g., `run_p7_code(contents, "main")`).

---

## 1.1 Packages, modules, imports, visibility

### 1.1.1 Packages

A **package** is the unit of compilation and dependency distribution.

- Package names are chosen by the host or tooling.
- The compiler accepts a package name and a set of source files (modules).
- A package may depend on other packages; dependencies are provided by host/tooling.

### 1.1.2 Modules

A **module** is a single source file.

- Each source file in a package is one module.
- A module has a **module path** derived from its file path within the package (host/tooling-defined mapping).
  - Recommended mapping: `/` becomes `.`.
  - Example: `src/util/string.p7` → `mypackage.src.util.string`.

### 1.1.3 Absolute module paths

An **absolute module path** begins with a package name and uses `.` as a separator.

Examples:
- `std.collections.list`
- `myapp.services.auth`

Qualified names may be used in any name position (types, expressions, etc.), except for leading-`.` relative paths which are restricted to `import` (§1.1.5).

### 1.1.4 Import statements

`import` brings a module into scope.

Syntax:
```p7
import <module-path>;
import <module-path> as <name>;
```

`import P;` binds the last segment of `P`.  
`import P as N;` binds `N`.

After import, the bound name refers to the imported module; its public (`pub`) declarations are available via that name.

Example:
```p7
import std.collections.list;
import myapp.services.auth as Auth;

list.new_list();
Auth.login();
```

### 1.1.5 Relative module paths (import-only)

A relative module path begins with `.` and is permitted **only** in `import`.

- `.foo` refers to a sibling module `foo`.
- `.sub.bar` refers to module `bar` in subdirectory `sub`.

Example:
```p7
// In module `myapp.services.auth`
import .helpers;          // `myapp.services.helpers`
import .sub.utilities;    // `myapp.services.sub.utilities`
```

### 1.1.6 Package-root relative imports (import-only)

A **package-root relative** import path begins with `_` followed by `.`, and is permitted **only** in `import`.

- `_.foo.bar` resolves to `<current-package-name>.foo.bar`.
- This form allows importing modules from the package root regardless of the importing module's depth.

Rules:
- `import _;` is ERROR (bare `_` is not a valid module path).
- `_.…` may **only** be used in `import` statements; it is not permitted in qualified names for types or expressions.

Example:
```p7
// In module `mypackage.services.auth.handlers`
import _.util.logging;      // resolves to `mypackage.util.logging`
import _.config;            // resolves to `mypackage.config`
```

### 1.1.7 Visibility

By default, declarations are module-private.

- A declaration marked `pub` is visible outside the module.
- Module-private declarations are visible only within the same source file.

Example:
```p7
// in `myapp.util.helpers`
pub fn public_helper() { ... }
fn private_helper() { ... }
```

From another module:
```p7
import myapp.util.helpers;

helpers.public_helper();   // ok
helpers.private_helper();  // ERROR
```

---

## 2. Lexical structure

### 2.1 Identifiers
Identifiers start with `_` or a letter and continue with letters, digits, or `_`.

### 2.2 Keywords and identifiers

**Reserved keywords** (minimal set):  
`fn`, `struct`, `enum`, `proto`, `let`, `var`, `pub`, `return`, `if`, `else`, `loop`, `break`, `continue`, `for`, `in`, `ref`, `import`, `as`, `box`, `robox`

`true` and `false` are **keywords** (boolean literals).  
`null` is a keyword (null literal).

**Contextual keywords** (not reserved; allowed as identifiers in most contexts):  
`throw`, `try`, `yield`

These keywords have special meaning only in specific syntactic positions (e.g., `throw` in statement position within a function with the `throws` effect; `try` in expression position; `yield` in statement position within a function with the `suspend` effect). Elsewhere, they may be used as ordinary identifiers.

**Effect identifiers** (used in function effect sets):  
`throws`, `suspend`

These identifiers are recognized in the effect syntax `fn[effect1, effect2, ...]` to declare function effects. They are not reserved as keywords and may be used as ordinary identifiers in other contexts.

[[TODO]] confirm final keyword set; keep minimal.

### 2.3 Comments
- Line comments: `// ...`
- Block comments: `/* ... */`

### 2.4 Syntactic Shorthands (Sigils)

To facilitate rapid authoring without sacrificing the auditability of the final code, the compiler accepts specific symbols (sigils) as synonyms for core keywords.

**Canonicalization Rule:**
While the compiler accepts these sigils, standard formatters and linters are encouraged to replace them with their keyword equivalents (`ref`, `box`) in stored source files to maximize readability for reviewers.

| Sigil | Keyword Equivalent | Meaning | Usage (Type) | Usage (Expr) |
| :--- | :--- | :--- | :--- | :--- |
| **`&`** | `ref` | Borrowed View | `x: &T` $\to$ `x: ref<T>` | `&x` $\to$ `ref(x)` |
| **`^`** | `box` | Owned Handle | `x: ^T` $\to$ `x: box<T>` | `^x` $\to$ `box(x)` |
| **`?`** | `?` | Nullable | `x: ?T` $\to$ `x: ?T` | N/A |

**Sigil Usage Rules:**
1.  **Type Position:** Sigils may replace the generic type wrapper.
    *   `&^int` is equivalent to `ref<box<int>>`.
    *   `?^string` is equivalent to `?box<string>`.
2.  **Expression Position:** Sigils act as prefix operators.
    *   `let r = &x;` is equivalent to `let r = ref(x);`.
    *   `let b = ^10;` is equivalent to `let b = box(10);`.
    *   `let b = ^(10);` is equivalent to `let b = box(10);`.

**Rationale for `^` (Caret):**
The `^` symbol visually suggests a "pointer" or "handle" (pointing up to the heap). The `@` symbol is reserved for Attributes (§19).

---

## 3. Types (overview)

Types in v1:
- Primitive: `int`, `float`, `bool`, `char`, `unit`, `ptr`
- Built-in value types: `string`, `array<T>`, tuples `(T1, T2, ...)`
- Nullability: `?T`
- Borrowed view: `ref<T>` (Input: `&T`)
- Owned heap handle: `box<T>` (Input: `^T`)
- Read-only heap handle: `robox<T>`
- User-defined: `struct Name(...)`, `enum Name(...)`, `proto Name { ... }`
- Compile-time generics: `T`, `array<T>`, `box<T>`, `robox<T>`, etc. (§20)

---

## 3.1 Primitive types

- `int`  
  Signed 64-bit two's-complement integer (i64). Integer overflow TRAPs (§15.1.1).

- `float`  
  IEEE-754 binary64 (f64). NaN/Inf behavior follows [IEEE-754](https://en.wikipedia.org/wiki/IEEE_754).
  - NaN is unordered: `x == NaN` is false, `x != NaN` is true, and all ordered comparisons with NaN are false.
  - Arithmetic propagates NaN and infinities per IEEE-754.

- `bool`  
  Boolean. Values: `true`, `false`.

- `char`  
  A Unicode scalar value (Unicode code point excluding surrogates).  
  Literal syntax (v1):
  - Single-quoted: `'a'`
  - Escapes: `\n`, `\r`, `\t`, `\0`, `\'`, `\\`
  - Unicode scalar escape: `\u{...}` with 1–6 hex digits, value must be a Unicode scalar (no surrogates).

- `unit`  
  The unit type with a single value written `()`.

- `ptr`  
  A raw, pointer-sized machine address.  
  Properties:
  - Non-null by default; nullable version is `?ptr`.
  - Copy-eligible and Copy-treated.
  - Allowed operations (v1): `==` and `!=`.
  - Other operations (arithmetic, dereferencing) are TODO and expected only under FFI/unsafe extension.

---

## 3.2 `string`

- `string` is a built-in **immutable value type** containing UTF-8 text.
- Iteration unit is `char`.

Minimum v1 operations (exact spelling may be in a prelude/stdlib; these names are normative placeholders):
- `len_chars(s: string) -> int`  
- `get_char(s: string, i: int) -> ?char` (0-based; out of bounds → `null`)

Indexing policy:
- No `s[i]` syntax for strings in v1.

String literal syntax and escapes are defined in §4.3.

[[TODO]] concatenation spelling, slicing APIs.

---

## 3.3 `array<T>`

- `array<T>` is a built-in **immutable value type**.
- In-place mutation of a value array is not supported in v1.
- Shared mutation/identity is provided via `box<array<T>>` with mutation APIs (§7.4, §3.3.3).

### 3.3.1 Array literals

- `[e1, e2, ...]` constructs an `array<T>` where all elements have the same inferred type `T`. The element type is synthesized from the element expressions.
- `[]` (empty array literal) requires an expected type to determine `T`; otherwise ERROR.
  - Example: `let ys: array<string> = [];` is OK (expected type provides `T=string`).
  - Example: `let xs = [];` is ERROR (no elements, no expected type).

No implicit numeric widening inside array literals.

Example:
```p7
let xs = [1, 2, 3];              // OK: synthesizes array<int>
let ys: array<string> = [];      // OK: expected type provides T=string

fn get_empty() -> array<float> {
  return [];                     // OK: return type provides T=float
}

// let zs = [];                  // ERROR: cannot infer T (no elements, no context)
```

### 3.3.2 Array indexing

Two indexing forms:

1) **Trap indexing**:
- `a[i]` reads element at index `i` (0-based).
- If `i` is negative or out of bounds, evaluation TRAPs.

2) **Checked indexing**:
- `a.get(i)` returns `?T`.
- If `i` is negative or out of bounds, returns `null`.

[[TODO]] define full array API surface (`len`, `get`, etc.) and whether `get` is syntax sugar for a prelude function.

### 3.3.3 Boxed array mutation (overview)

Mutation of an array requires boxing:
- `box<array<T>>` represents a mutable, identity-bearing container.

v1 boxed-array mutation is via library operations (not indexing assignment). [[TODO]] specify API (e.g. `push`, `set`, `pop`) and their signatures.

---

## 3.4 Tuple types

Tuples are built-in **immutable value types** that group multiple values of potentially different types.

### 3.4.1 Tuple type syntax

A tuple type is written as `(T1, T2, ..., Tn)` where `n >= 2`.

Examples:
- `(int, string)` — a 2-tuple (pair) of an `int` and a `string`
- `(float, float, float)` — a 3-tuple of three `float` values
- `(int, (string, bool))` — nested tuples are allowed

Special cases:
- `()` is the **unit type** (not a tuple), with a single value `()`.
- `(T)` is **not** a tuple type; it is interpreted as a parenthesized type expression (i.e., just `T`).

### 3.4.2 Tuple literals

A tuple literal is written as `(e1, e2, ..., en)` where `n >= 2`.

Examples:
```p7
let pair = (1, "hello");           // type: (int, string)
let triple = (3.14, true, 42);     // type: (float, bool, int)
let nested = (1, ("a", false));    // type: (int, (string, bool))
```

Special cases:
- `()` is the **unit literal** (not a tuple literal).
- `(e)` is **not** a tuple literal; it is a parenthesized expression (grouping).

### 3.4.3 Element access

Tuple elements are accessed using dot notation with zero-based integer indices:
- `t.0` accesses the first element
- `t.1` accesses the second element
- `t.N` accesses the element at index `N`

Example:
```p7
let p = (42, "test");
let x = p.0;  // x has type int, value 42
let y = p.1;  // y has type string, value "test"
```

### 3.4.4 Structural rules

- Tuple types are **Copy-treated** when all component types are Copy-treated.
- Tuple types are **Send** when all component types are Send.
- Tuple elements cannot be mutated in-place (tuples are immutable value types).

---

## 3.5 Nullable types: `?T`

- `?T` is either `null` or a non-null `T`.
- `null` is assignable only to `?T`.
- Unwrapping and narrowing rules are in §15.2.

---

## 3.6 Borrowed view types: `ref<T>`

`ref<T>` is a **read-only view** of an existing addressable location that holds a `T` (§7).

- `ref<T>` values are **Copy-treated** (copying a `ref<T>` copies the view/handle; it does not copy the underlying `T`).
- `ref<T>` values are **non-escapable** (§7.3).

`ref<?T>` is permitted and means a view of a nullable location.

---

## 3.7 Owned heap handle types: `box<T>`

`box<T>` is an **owned heap-allocated identity container** holding a `T`.

- `box<T>` values can escape (stored, returned, captured, interop).
- `box<T>` is **Copy-treated**: copying a box copies the handle; all copies alias the same boxed cell.
- Mutation of boxed contents is visible through all aliases.

---

## 3.8 Read-only heap handle types: `robox<T>`

`robox<T>` is a **read-only heap-allocated identity container** holding a `T`.

- `robox<T>` values can escape (stored, returned, captured, interop).
- `robox<T>` is **Copy-treated**: copying a robox copies the handle; all copies alias the same boxed cell.
- `robox<T>` **forbids mutation** through the handle:
  - `*rb = ...` is ERROR when `rb: robox<T>`.
  - `rb.field = ...` is ERROR when `rb: robox<S>`.
- `robox<T>` supports borrowing boxed contents with `ref(*rb)`.
- Dereferencing `*rb` as a value is allowed only when `T` is Copy-eligible; otherwise ERROR (mirroring the `box<T>` rule).
- Method-call behavior:
  - Calling methods with `ref self` receivers on `robox<Self>` is allowed via auto-borrow to `ref(*rb)` (§11.3.1).
  - Calling `box self` methods on `robox<Self>` is ERROR.

**Relationship to `box<T>`:**

- `box<T>` may implicitly coerce to `robox<T>` (capability-weakening) in checking/expected-type contexts:
  - Assignment to an annotated `robox<T>` type.
  - Argument passing to a `robox<T>` parameter.
  - Function return when the return type is `robox<T>`.
  - Contextual branch/join with expected type `robox<T>`.
- The reverse coercion `robox<T> -> box<T>` is **not** allowed (ERROR). There is no v1 mechanism for downcast.

---

## 4. Values and literals

### 4.1 Integer literals
Decimal digits with optional `_`: `0`, `42`, `1_000_000`

### 4.2 Float literals
Decimal with `.` and optional `_`: `1.0`, `3.1415`, `1_000.5`  
[[TODO]] exponent notation.

### 4.3 String literals
Double-quoted strings: `"hello"`

String literals MUST NOT contain unescaped newlines.

Escape sequences:
- `\\` (backslash)
- `\"` (double quote)
- `\n` (newline)
- `\r` (carriage return)
- `\t` (tab)
- `\0` (NUL)
- `\u{...}` Unicode scalar escape with 1–6 hex digits; the value MUST be a Unicode scalar (no surrogates).

Any other `\`-escape sequence is an ERROR.

### 4.4 Boolean literals
`true`, `false`

### 4.5 Null literal

`null` requires an expected type `?T` to determine the underlying type `T`; otherwise ERROR.

The `null` literal is checked against the expected type (§0.1) and cannot synthesize a type on its own.

Examples:
```p7
let x: ?int = null;              // OK: expected type provides ?int
let y: ?string = null;           // OK: expected type provides ?string

fn maybe_int() -> ?int {
  return null;                   // OK: return type provides ?int
}

// let z = null;                 // ERROR: cannot infer T for ?T (no context)
```

---

## 5. Bindings, shadowing, and mutation

### 5.1 `let` and `var` bindings (slots)

`let x = expr;` introduces an immutable slot.

- `let` slots are single-assignment.
- Assigning to a `let` slot (e.g., `x = expr`) is ERROR.

`var x = expr;` introduces a mutable slot (v1).

- `var` slots can be reassigned: `x = new_expr;` where `new_expr` has the same type as the slot.
- `var` slots are mutable but NOT addressable (see §0); borrowing via `ref(x)` where `x` is a `var` slot is ERROR.

### 5.2 Shadowing

A `let` or `var` may introduce a new binding with the same name as an outer binding.

Rule: if `x` shadows `x`, the new binding MUST have the **same type** as the shadowed binding. The mutability may differ (i.e., a `let` binding may shadow a `var` binding and vice versa).

Example:
```p7
let a = 1;
{
  let a = 2;  // ok: same type int
  a
}
a
```

Example with `var`:
```p7
let x = 10;
{
  var x = 20;  // ok: same type int, but now mutable
  x = 30;      // ok: x is var
}
x  // still 10; outer x is immutable
```

### 5.3 Mutation and identity

Protosept supports two forms of mutation:

1. **Local-only slot reassignment** (v1): `var` slots can be reassigned (§5.1). This is purely local mutation; `var` slots cannot be borrowed via `ref(...)`.

2. **Shared identity mutation**: In-place mutation through `box<T>`.
   - Assigning to a field is allowed only through a box:
     - `p.x = 1` is valid only if `p: box<Point>`.
   - Value structs and value arrays are immutable.

The distinction ensures that shared/observable mutation is always expressed via `box<T>`, while `var` provides convenient local reassignment for loop counters, accumulators, and similar use cases.

[[TODO]] finalize exact `*b = ...` syntax and boxed-array mutation APIs (see §7.4, §3.3.3).

#### Example: `var` in a loop accumulator

```p7
fn sum(arr: array<int>) -> int {
  var total = 0;
  for x in arr {
    total = total + x;  // ok: total is var
  }
  total
}
```

#### Example: `var` slots cannot be borrowed

```p7
var count = 0;
let r = ref(count);  // ERROR
```

This is ERROR because `var` slots are not addressable locations.

---

## 6. Moves, copies, and `copy(x)`

### 6.1 Value-flow rule (move-by-default)

Whenever a value flows into a new slot or output position (binding, parameter, return, break-value, etc.):

- If the type is **Copy-treated**, the value is copied.
- Otherwise, the value is moved and the source becomes invalid to use (ERROR if used).

This rule applies uniformly to:
- `let` bindings
- argument passing
- returns
- `break expr` values
- iteration bindings (`for`)

### 6.2 Copy-treated vs Copy-eligible

p7 distinguishes:

- **Copy-eligible** (structural conformance to `Copy`): the type can be duplicated safely by duplicating its parts.
- **Copy-treated** (implicit behavior): the compiler chooses copying at value-flow sites.

Copy-eligible is a structural property; Copy-treated is an *implicit* behavior.

### 6.3 The `Copy` proto (constraint proto)

`Copy` is a built-in **constraint proto** used for structural eligibility checks.

Copy-eligible (structural `T: Copy`) in v1:
- Primitives: `int`, `float`, `bool`, `char`, `unit`
- `string`
- `box<T>` (handle copy)
- `ref<T>` (view/handle copy)
- `?T` iff `T` is Copy-eligible
- `array<T>` iff `T` is Copy-eligible
- `struct` iff all fields are Copy-eligible
- `enum` iff all payload field types are Copy-eligible

Copy-treated by default in v1:
- Primitives
- `string`
- `box<T>`
- `robox<T>`
- `ref<T>`
- `?T` iff `T` is Copy-treated (by composition)
- User-defined `struct` types are Copy-treated **only if** they opt in via `struct[Copy] ...`.
- User-defined `enum` types are Copy-treated **only if** they opt in via `enum[Copy] ...` 

### 6.4 Explicit copying: `copy(x)`

`copy(x)` is the explicit copying operation.

- `copy(x)` is well-typed iff the type of `x` is Copy-eligible (`T: Copy`).
- It returns a value of the same type as `x`.
- It does not require the type to be Copy-treated.

Rationale:
- Structural properties are usable explicitly (`copy`), while implicit duplication can be gated by opt-in for user types.

---

## 6.5 The `Send` constraint proto

`Send` is a built-in constraint proto indicating a deep-copyable pure value with no shared identity/aliasing.

Send-eligible in v1:
- Primitives
- `string`
- `array<T>` iff `T` is Send-eligible
- User-defined `enum` iff all payload field types are Send-eligible
- User-defined `struct` iff all fields are Send-eligible

Not Send-eligible:
- `box<T>`
- `robox<T>`
- `ref<T>`
- Any type transitively containing `box<...>`, `robox<...>`, or `ref<...>`

`Send` is primarily used by the Threading extension (§22), but is always available in the core language.

---

## 7. Borrowed views (`ref<T>`), boxes (`box<T>`), and read-only boxes (`robox<T>`)

### 7.1 Meaning of ref<T> (Input: &T)

A value of type `ref<T>` is a read-only view of an addressable location holding a `T`.

- Dereference: `*r` reads the current value of the referent location.
- Read semantics: `*r` yields a value of type `T`:
  - if `T` is Copy-treated, `*r` yields a copy;
  - otherwise, `*r` causes an ERROR.

**Operations on `ref<T>` values:**

- Member access (`r.field`) and method calls (`r.method(...)`) are permitted without copying `T`:
  - These operations access the referent location directly.
  - Only explicit dereference (`*r`) is restricted by the Copy-treated requirement.

### 7.2 Taking views

`ref(place)` produces a `ref<T>` when `place` is an addressable location of type `T`.

Requirements:
- `place` MUST be an addressable location (see §0). Note that `var` slots are not addressable.

In v1, borrowing is always explicit:
- There is no implicit borrowing at call sites (except for method-call auto-borrow sugar; see §11.3.1).

### 7.3 Non-escapable rule (hard rule in v1)

Values of type `ref<T>` MUST NOT escape their scope.

A `ref<T>` value MUST NOT be:
- returned from a function
- stored into a struct field
- stored into an array element
- stored into any heap-allocated value (including inside `box<...>`)
- stored in globals/statics
- captured by closures (if/when closures exist)
- passed across host interop boundaries as a persistent value

Consequences:
- User-defined types MUST NOT contain `ref<...>` fields.
- `array<ref<T>>` is ERROR.

**Example (ref cannot be stored in heap):**
```p7
let x = 42;
let r = ref(x);
let b = box(r);  // ERROR: cannot store ref<T> in box<...>
```

### 7.4 Meaning of box<T> (Input: ^T)

A `box<T>` is an identity-bearing heap cell containing a `T`.

Operations (surface syntax v1):

- **Construction (Explicit Allocation):** Allocation is always explicit.
  - Canonical: `box(expr)` allocates a new boxed cell containing `expr`.
  - Shorthand: `^expr`
  - **Desugaring**: `box(expr)` desugars to `box<T>.new(expr)` where `T` is the type of `expr`. `box<T>.new` is an intrinsic method declared in the prelude. [[TODO]] specify prelude location/definition of `box<T>.new` (§23).

- Read / deref: `*b`
  - `*b` is an **addressable location** (place expression) referring to the boxed contents.
  - If `T` is Copy-eligible, `*b` as a value expression yields a copy of type `T`.
  - If `T` is not Copy-eligible, using `*b` as a value expression (moving out) is ERROR in v1.
  - Rationale: boxes are aliasable; moving out implicitly would require moved-out states or uniqueness.

- Write: `*b = expr`
  - Requires `expr: T`.
  - Overwrites the boxed contents.

- Replace: `replace(b, new_value) -> T`
  - Writes `new_value` into the box and returns the previous value.
  - This is the way to extract non-Copy values from a box without leaving it uninitialized.

- **Borrowing boxed contents**: `ref(*b)`
  - Produces a `ref<T>` view of the boxed contents.
  - Permitted for **any** `T`, including non-Copy-treated types.
  - The resulting `ref<T>` follows all `ref<...>` rules (§7.1, §7.3).
  - When `T` is an object proto `P`, `ref(*b)` yields `ref<P>` and is dynamically dispatched per §18.

- Member auto-deref:
  - `b.field`, `b.method(...)` act as if on the inner `T`.
  - Field assignment is allowed for boxed structs: `b.field = expr` updates the inner field in place (requires `b: box<S>`).

### 7.5 Meaning of robox<T>

A `robox<T>` is a **read-only** identity-bearing heap cell containing a `T`.

Operations (surface syntax v1):

- **Construction:** `robox<T>` values are typically obtained via coercion from `box<T>` (see below). There is no direct `robox(expr)` syntax; construction requires first creating a `box<T>` and coercing it.

- Read / deref: `*rb` (where `rb: robox<T>`)
  - `*rb` is **not** a mutable place; it cannot appear on the left side of assignment.
  - If `T` is Copy-eligible, `*rb` as a value expression yields a copy of type `T`.
  - If `T` is not Copy-eligible, using `*rb` as a value expression is ERROR (mirroring the `box<T>` rule).

- Write: `*rb = expr` is **ERROR** when `rb: robox<T>`.

- **Borrowing boxed contents**: `ref(*rb)`
  - Produces a `ref<T>` view of the boxed contents.
  - Permitted for **any** `T`, including non-Copy-treated types.
  - The resulting `ref<T>` follows all `ref<...>` rules (§7.1, §7.3).
  - When `T` is an object proto `P`, `ref(*rb)` yields `ref<P>` and is dynamically dispatched per §18.

- Member auto-deref:
  - `rb.field` reads the field (no assignment allowed).
  - `rb.field = expr` is **ERROR** when `rb: robox<S>`.
  - `rb.method(...)` is allowed when the method has a `ref self` receiver (desugars to `Type.method(ref(*rb), ...)` per §11.3.1).
  - Calling methods with `box self` receivers on `robox<Self>` is ERROR.

**Coercion from `box<T>` to `robox<T>`:**

In **checking/expected-type contexts**, a `box<T>` value may implicitly coerce to `robox<T>` (capability-weakening):

- Assignment: `let rb: robox<T> = b;` where `b: box<T>`.
- Parameter passing: `f(b)` where `f` expects `robox<T>` and `b: box<T>`.
- Return: `return b;` where the function return type is `robox<T>` and `b: box<T>`.
- Branch/join: if/else branches with expected type `robox<T>` may return `box<T>` expressions.

The reverse coercion `robox<T> -> box<T>` is **not** allowed (ERROR).

**Rationale:**

`robox<T>` provides a type-safe mechanism to share heap-allocated values without permitting mutation, enabling safer API boundaries and immutable views of mutable data.

---

## 8. Runtime failures: traps vs throws

Protosept has two failure channels:

1) **TRAPs (panics)**: unrecoverable runtime failures representing bugs/contract violations.
   - Examples: integer overflow, out-of-bounds `a[i]`, force unwrap of `null` (`x!`).
   - TRAPs cannot be caught by `try`.
   - TRAPs propagate to the host as an unrecoverable failure outcome.

2) **THROWs (typed errors)**: recoverable failures represented by enum values.
   - THROWN values can be handled or propagated using `try` (§14).
   - The type system tracks which functions may throw via the `throws` or `throws<E>` effect (§11.2).

Host-visible outcomes of calling a Protosept function are:
- Returned(value)
- Threw(enum_value)
- Trapped(panic)

---

## 9. Expressions

Expressions include:
- literals
- identifiers
- unary/binary operations
- calls
- field access
- block expressions
- `if` expressions
- `loop` expressions
- `try` expressions
- `match` expressions

`yield` is only available under the Fiber extension (§21).

### 9.1 Block expressions

A block `{ ... }` contains statements.

Block value:
- If the final statement is an expression statement without trailing `;`, the block evaluates to that expression’s value.
- Otherwise it evaluates to `()` (unit).

### 9.2 `if` expression and statement

Two forms:

1) **`if` with `else` (expression form)**:
```p7
if condition { then_block } else { else_block }
```

2) **`if` without `else` (statement form)**:
```p7
if condition { then_block }
```

Rules:
- Braces are mandatory around both `then_block` and `else_block`.
- `condition` MUST be `bool`.

**Expression form** (`if ... else ...`):
- The `if` expression yields the value of the evaluated branch.
- `then_block` and `else_block` MUST have identical types in v1.
- The `if` expression has the same type as the branches.
- May be used in any expression position (e.g., assignment, return, nested in other expressions).

**Statement form** (`if` without `else`):
- Permitted only in statement position (not in expression contexts).
- `then_block` MUST have type `unit` or ERROR.
- Does not yield a value.

**Control flow**:
- `break`, `continue`, `return`, and `throw` statements inside `if` blocks behave as usual according to their enclosing control structures or functions.

### 9.3 Operators and precedence
Operator precedence (highest to lowest):

1) Postfix / primary
- Call: `()`
- Member access: `.`
- Trap indexing: `[]`
- Force unwrap: postfix `!`

2) Prefix unary
- `*` (deref)
- Unary `-`, unary `+`
- Logical NOT: `!`

3) Multiplicative: `*`, `/`, `%`

4) Additive: `+`, `-`

5) Comparisons: `<`, `<=`, `>`, `>=`

6) Equality: `==`, `!=`

7) Logical AND: `&&`

8) Logical OR: `||`

9) Null-coalescing: `??`

10) Assignment: `=` (right-associative). Assignment is a statement form; it does not yield a value.

Notes:
- `if ... else ...`, `try`, `match`, and `loop` are expression forms written with blocks and bind looser than any operator above.
- `if` without `else` is statement-only and does not participate in operator precedence.
- Prefix `!x` (logical NOT) and postfix `x!` (force unwrap) are distinct by position.

### 9.4 `loop` expressions

Two forms:

1) Infinite loop:
```p7
loop { body }
```

2) Loop with a single carried state binding:
```p7
loop (let name = init; let name = step) { body }
```

Rules:
- `init` MUST be exactly one `let` binding.
- `step` MUST be exactly one `let` binding.
- `step` MUST bind the same `name` as `init`.

Semantics:
- `init` runs once before the first iteration.
- Each iteration evaluates `body`.
- After a normal iteration (not `break`), `step` runs and becomes the binding for the next iteration.
- `break` exits the loop; `break expr` yields a value.

Control flow:
- `break;` yields `()`.
- `break expr;` yields `expr`.
- `continue;` starts the next iteration (and executes `step` if present).

Type rule:
- If any `break expr;` occurs, all break values MUST have identical type in v1.
- Otherwise the `loop` type is `unit`.

Borrow interaction:
- Each iteration’s `let` creates a fresh slot; any `ref` taken to that slot is confined by §7.3.

### 9.5 `while` statement (v1)

Form:
```p7
while condition { body }
```

Rules:
- `condition` MUST be `bool`.
- `while` is a statement that yields `unit` when used in a block.

Semantics:
- `condition` is evaluated before each iteration.
- If `condition` is `true`, `body` executes and control returns to evaluate `condition` again.
- If `condition` is `false`, the loop exits.

Control flow:
- `break` and `continue` behave as in `loop`.
- `break;` exits the loop and yields `()`.
- `break expr;` exits the loop and yields `expr`.
- `continue;` skips to the next iteration (re-evaluates `condition`).

**Normative desugaring**:

The `while` statement is defined by desugaring to `loop`:
```p7
while condition { body }
```
desugars to:
```p7
loop {
  if condition { body } else { break; }
}
```

This desugaring is normative; implementations MAY optimize but MUST preserve the observable semantics of this desugaring, including the behavior of `break` and `continue` within `body`.

---

### 9.6 `match` expression

`match` selects the first matching arm from an ordered list.

Form:
```p7
match scrutinee {
  pattern1 => expr1,
  pattern2 => expr2,
  ...
}
```

Arm separator:
- Arms are separated by `,`.
- A trailing comma is permitted.
- Each `expr` may be any expression, including a block.

#### 9.6.1 Patterns (v1)

In v1, patterns are **value patterns** (equality tests), optionally with a binding.

Grammar sketch:
```
named_pattern := [name ':'] pattern
pattern       := literal | path
path          := ident ('.' ident)*
literal       := int_lit | float_lit | string_lit
```

Supported pattern forms:
- **Wildcard**: `_` matches any value.
- **Literal patterns**: `42`, `3.14`, `"hi"` match values equal to that literal.
- **Path patterns**: `EnumName.VariantName` (and longer qualified paths) match values equal to that path’s value.
  - For enums, path patterns are valid only for **unit variants** in v1. Payload variants cannot be matched with path patterns alone.
- **Named binding**: `name: pattern` binds `name` to the scrutinee value **when the arm matches**, then evaluates the arm expression.
  - Commonly used with wildcard: `name: _`.

Notes:
- v1 does not support payload destructuring such as `Result.Ok(x) => ...` or `Result.Ok(_) => ...`.
- Implementations may further restrict which types are valid scrutinee/pattern types based on available equality semantics.

#### 9.6.2 Evaluation and control flow

- The `scrutinee` expression is evaluated exactly once.
- Arms are tried in source order.
- For each arm:
  - If `pattern` matches, the arm’s binding (if present) is introduced, then the arm expression is evaluated and becomes the result of the `match`.
  - If `pattern` does not match, the next arm is tried.

#### 9.6.3 Typing

- All arm expressions MUST have the same type in v1.
- The `match` expression has that common type.

#### 9.6.4 Exhaustiveness (v1)

`match` MUST be exhaustive.

- If it is not statically provable that some arm matches, the program is ill-formed (ERROR).
- The simplest portable way to be exhaustive is to include a final wildcard arm `_ => ...`.

Example:
```p7
fn classify(x: int) -> int {
  match x {
    0 => 0,
    n: _ => n,
  }
}
```

---

## 10. Statements

Statement forms:
- `let` binding: `let x = expr;`
- `var` binding: `var x = expr;`
- expression statement: `expr;`
- `if` statement (§9.2): `if condition { then_block }`
- `while` statement (§9.5): `while condition { body }`
- `for` statement (§10.3)
- `return;` or `return expr;`
- `throw expr;` (only in functions with `throws` or `throws<E>` effect; §14)
- `break;` / `break expr;`
- `continue;`
- assignment statement (§10.2)
- `yield;` (Fiber extension only; §21)
- declarations where allowed [[TODO]] (recommended: declarations only at top-level in v1)

### 10.1 Returns

A function returns:
- the argument of `return expr;`, or
- the last expression value in the function body block if it is not terminated by `;`, otherwise `()`.

### 10.2 Assignment statement

Form:
```p7
place = expr;
```

Rules:
- `place` may be:
  1. A `var` slot: `x = expr` where `x` was introduced by `var`.
     - `expr` MUST have the same type as the slot.
  2. An addressable location that is mutable by definition:
     - boxed deref: `*b = expr` where `b: box<T>`
     - boxed field: `b.field = expr` where `b: box<S>`
- Assignment to read-only boxes is ERROR:
  - `*rb = expr` where `rb: robox<T>` is ERROR.
  - `rb.field = expr` where `rb: robox<S>` is ERROR.
- Assigning to a `let` slot (`x = expr` where `x` was introduced by `let`) is ERROR.
- Assignment does not produce a value.

### 10.3 `for` statement (v1)

Form:
```p7
for x in expr { body }
```

`expr` MUST have type:
- `array<T>` (then `x: T`), or
- `string` (then `x: char`).

Semantics:
- `expr` is evaluated once.
- `body` executes once per element/character, in order.
- `break` and `continue` behave as in `loop`.

Binding rule:
- `x` is a fresh binding each iteration.

---

## 11. Functions

### 11.1 Declarations

Core form:
```p7
fn name(p1: T1, p2: T2, ...) -> R { ... }
```

- If `-> R` is omitted, return type is `unit`.

### 11.2 Function effects

**Canonical form** (v1):
```p7
fn[effect1, effect2, ...] name(p1: T1, p2: T2, ...) -> R { ... }
```

The effect set is specified in square brackets immediately after `fn`. Effects are a **set**:
- **Duplicates are ERROR**: `fn[throws, throws]` is invalid.
- **Order is not semantically significant**: `fn[throws, suspend]` and `fn[suspend, throws]` are equivalent.

**v1 effect identifiers** (closed set):
- `throws`: may throw any enum value
- `throws<E>`: may throw only values of enum type `E` (parameterized effect)
- `suspend`: may suspend via `yield` (Fiber extension; §21)

Examples:
```p7
fn[throws] read_file(path: string) -> string { ... }
fn[throws<FileError>] safe_read(path: string) -> string { ... }
fn[suspend] fiber_task() { ... }
fn[throws, suspend] async_read(path: string) -> string { ... }
```

### 11.3 Parameter passing

At call sites, argument passing uses the value-flow rule (§6.1).

For `ref<T>` parameters:
- Caller MUST pass an addressable location explicitly with `ref(...)`:
  - `f(ref(x))`.

Mutation requires `box<T>` parameters.

For `robox<T>` parameters:
- A `box<T>` argument may be passed and will implicitly coerce to `robox<T>` (§7.5).

#### 11.3.1 Method-call auto-borrow sugar for `ref self` receivers

For method calls only, p7 provides auto-borrow sugar when the method has a `ref self` receiver:

**Sugar rules:**

- `recv.method(args...)` where `method` has a `ref self` receiver desugars as follows:
  - If `recv` has type `Self` and is an addressable location: desugars to `Type.method(ref(recv), args...)`.
  - If `recv` has type `box<Self>`: desugars to `Type.method(ref(*recv), args...)`. The receiver `recv` may be any value (including temporaries), as the borrow is taken of the dereferenced contents `*recv`.
  - If `recv` has type `robox<Self>`: desugars to `Type.method(ref(*recv), args...)`. The receiver `recv` may be any value (including temporaries), as the borrow is taken of the dereferenced contents `*recv`.
  - If `recv` already has type `ref<Self>`: it is passed directly to the `ref self` parameter without desugaring.
  - The receiver is evaluated exactly once.

**Restrictions:**

- Applies only to methods with `ref self` receivers; does NOT apply to free functions with `ref<T>` parameters.
- When the receiver has type `Self`, it MUST be an addressable location; borrowing of temporaries is not permitted.

**Rationale:**

This sugar reduces ceremony at method call sites while maintaining explicit borrowing for free function calls, providing ergonomics where method chaining and fluent APIs are common.

### 11.4 Method receivers (v1)

Methods on structs, enums, and protos may declare a receiver parameter. The receiver is the first parameter and uses special syntax.

**Receiver forms:**

1. `self` – by-value receiver:
   - Type: `Self` (the declaring type).
   - Passes ownership; subject to value-flow rules (§6.1).

2. `self: ref<Self>` or shorthand `ref self` – borrowed receiver:
   - Type: `ref<Self>`.
   - Caller passes a read-only view of an addressable location.
   - Method-call syntax automatically applies the auto-borrow sugar (§11.3.1).

3. `self: box<Self>` or shorthand `box self` – boxed receiver:
   - Type: `box<Self>`.
   - Caller passes a boxed handle to the instance.
   - The boxed handle is Copy-treated (§6.2); passing does not move the box itself.
   - The box's contents remain aliased; multiple calls to methods on the same box see shared state.

**Rules:**

- The receiver is the first parameter; it is written before other parameters without a trailing comma.
- No implicit boxing occurs to satisfy a receiver:
  - A method with `self: box<Self>` requires the caller to have `box<Self>`, not just `Self`.
- For methods with `ref self` receivers, the auto-borrow sugar (§11.3.1) applies at method call sites for `box<Self>` and `robox<Self>` receivers.
- Boxed receivers (`self: box<Self>`) pass the box handle, which is Copy-treated. This allows multiple method calls on the same box without moving the box itself.
- Calling a method with a `box self` receiver on a `robox<Self>` value is ERROR (capability mismatch).

**Example:**
```p7
struct Counter(count: int) {
  pub fn increment(box self) {
    self.count = self.count + 1;
  }
  pub fn get(ref self) -> int {
    return self.count;
  }
}

let c = box(Counter(0));
c.increment();   // ok: box handle is Copy-treated, box is not moved
c.increment();   // ok: can call again
let n = c.get(); // ok: desugars to Counter.get(ref(*c)) per §11.3.1

let rc: robox<Counter> = c;
// rc.increment(); // ERROR: increment requires box self, but rc is robox<Counter>
let m = rc.get();  // ok: get only requires ref self, desugars to Counter.get(ref(*rc))
```

---

## 12. Structs

A struct is a product type with zero or more fields. Structs support two forms:

1. **Record structs** – fields have names (e.g., `x: int`).
2. **Tuple structs** – fields are unnamed, accessed by position (e.g., `int`).

A struct MUST declare its fields in a uniform manner: either all fields are named, or all fields are unnamed. Mixing named and unnamed fields in a single struct is ERROR.

### 12.1 Record struct declaration

Record structs use named fields:

```p7
struct Point(
  x: int,
  y: int,
);
```

Fields may have defaults:
```p7
struct Vec2(
  pub x: float = 0,
  pub y: float = 0,
);
```

Fields may be marked `pub` for public visibility (see §12.1.2).

#### 12.1.1 Field-level visibility

By default, struct fields are private (visible only within the declaring module).

A field may be marked `pub` to make it visible outside the module:

```p7
struct Vec2(
  pub x: float,
  pub y: float,
);
```

Field visibility controls:
- **Field access** (§12.4): `s.field` is ERROR if `field` is not visible at the access site.
- **Construction** (§12.3): Construction `S(...)` is ERROR if any field is not visible at the construction site, even if defaults exist for non-visible fields.

#### 12.1.2 Uniformity rule: all named or all unnamed

A struct's fields MUST be either all named or all unnamed; mixing is ERROR:

```p7
// OK: all named
struct Point(x: int, y: int);

// OK: all unnamed (tuple struct, see §12.2)
struct Pair(int, int);

// ERROR: mixing named and unnamed
struct Bad(x: int, float);  // not allowed
```

### 12.2 Tuple struct declaration

Tuple structs use unnamed fields:

```p7
struct Pair(int, int);
struct Triple(pub int, pub int, pub int);
struct Newtype(pub int);
```

Fields may be marked `pub` for public visibility (§12.1.1). Field visibility rules apply the same as for record structs.

Tuple structs are useful for:
- Newtype patterns: wrapping a single value with a distinct nominal type.
- Simple product types where field names add no clarity.

#### 12.2.1 Tuple struct field access

Tuple struct fields are accessed by position using `.0`, `.1`, `.2`, etc.:

```p7
let p = Pair(10, 20);
let x = p.0;  // 10
let y = p.1;  // 20
```

Field access is ERROR if the field is not visible at the access site (§12.1.1).

### 12.3 Construction

Construct a struct by calling the struct name:

**Record struct:**
- `Point(1, 2)` – positional arguments
- `Point(y = 2, x = 1)` – named arguments

**Tuple struct:**
- `Pair(10, 20)` – positional arguments only

[[TODO]] rule for mixing positional and named args (recommended: disallow in v1).

#### 12.3.1 Construction visibility restriction

Construction `S(...)` is allowed **only if all fields of `S` are visible** at the construction site.

If any field is not visible, construction is ERROR, **even if defaults exist** for non-visible fields.

**Rationale:** This enforces encapsulation. Types with private fields must provide public constructors (e.g., a `new` method) to control construction.

**Example: newtype with private field**

```p7
struct UserId(int);  // field is private

// ERROR: cannot construct UserId outside its module
// let id = UserId(42);

// Instead, provide a public constructor method:
struct UserId(int) {
  pub fn new(id: int) -> UserId {
    return UserId(id);  // OK: construction inside the module
  }
  pub fn value(ref self) -> int {
    return self.0;  // OK: field access inside the module
  }
}

// Usage:
let id = UserId.new(42);  // OK
let val = id.value();     // OK
// let x = id.0;          // ERROR: field not visible
```

### 12.4 Field access and assignment

**Field reads:**
- `s.x` (record struct) or `s.0` (tuple struct) is allowed when the field is visible at the access site.
- Field access is ERROR if the field is not visible.

**Field writes:**
- `s.x = v` is ERROR unless `s: box<S>` (mutation requires boxing).
- Field write is also ERROR if the field is not visible.

Example:
```p7
let p = box(Point(1, 2));
p.x = 10; // ok: p is boxed and x is visible
```

### 12.5 Methods

A struct may include a method block:

```p7
struct Vec2(
  pub x: float = 0,
  pub y: float = 0,
) {
  pub fn length(ref self) -> float { ... }
  pub fn scale(box self, factor: float) {
    self.x = self.x * factor;
    self.y = self.y * factor;
  }
}
```

Method receivers are defined in §11.4. Structs may use `self`, `ref self`, or `box self` receivers.

### 12.6 Builtin structs: `@builtin()`

A struct may be declared with the `@builtin()` attribute to indicate a compiler-defined, opaque nominal type:

```p7
@builtin()
struct Handle;
```

**Rules for `@builtin()` structs (v1):**

- The struct MUST NOT declare concrete fields in the source. Field declarations are not applicable to `@builtin()` structs.
- The struct is a nominal type with compiler-defined representation.
- Construction `Handle(...)` is ERROR unless provided by the compiler via intrinsics or FFI.
- Field access `h.field` is not applicable (no fields are accessible).
- Methods may be declared with signature-only declarations using `@intrinsic()` or `@ffi(...)` (§19).

**Example:**

```p7
@builtin()
struct FileHandle {
  @intrinsic()
  pub fn close(self) -> unit;
}
```

**Rationale:** `@builtin()` structs allow the compiler to define opaque types for FFI, runtime handles, or platform-specific types without exposing internal representation.

---

## 13. Enums

### 13.1 Declaration

Enums are sum types with one or more named variants. Each variant can be:
- A **unit variant** (no associated data), or
- A **payload variant** (with associated data of specified types).

#### Syntax

Enum declarations mirror struct syntax using parentheses:

**No-method form** (ends with `;`):
```p7
enum EnumName(
  UnitVariant,
  PayloadVariant: Type,
  ...
);
```

**Method form** (includes method block):
```p7
enum EnumName(
  UnitVariant,
  PayloadVariant: Type,
  ...
) {
  fn method_name(...) -> ... { ... }
}
```

Variant syntax:
- **Unit variant**: `VariantName` (no type annotation)
- **Payload variant**: `VariantName: Type` for single-field payloads
- **Multi-field payload variant**: `VariantName: (Type1, Type2, ...)` using tuple types

Example with unit variants only:
```p7
enum SomeErrors(
  NumberIsNot42,
  DivisionByZero,
);
```

Example with both unit and payload variants:
```p7
enum Status(
  Pending,
  Active: int,
  Failed: (string, int),
);
```

### 13.2 Variant naming and construction

Variants are always referenced qualified by the enum name:
- `EnumName.VariantName`

#### Construction syntax

- **Unit variant**: `EnumName.VariantName`
  ```p7
  let e = SomeErrors.NumberIsNot42;
  ```

- **Payload variant**: `EnumName.VariantName(e1, e2, ...)`
  ```p7
  let s1 = Status.Active(42);
  let s2 = Status.Failed("connection error", 500);
  ```

#### Typing rules

- Each argument in a payload variant construction must match the corresponding payload field type.
- The number of arguments must exactly match the number of payload fields; otherwise ERROR.
- The result has type `EnumName`.

#### Restrictions

v1 does not support:
- Variant introspection operators (`is`)
- Payload extraction operators (`as?`)

These may be added in future versions.

### 13.3 Methods

An enum may include a method block, using the same method syntax as structs.

Method receivers are defined in §11.4. Enums may use `self`, `ref self`, or `box self` receivers.

This enables enums to satisfy object protos structurally (§18).

Example:
```p7
enum Option<T>(
  None,
  Some: T,
) {
  pub fn is_some(ref self) -> bool { ... }
}
```

---

## 14. Error handling: `throw` and `try`

### 14.1 `throw`

`throw expr;` aborts evaluation and transfers control to the nearest enclosing `try`.

Rules:
- `expr` MUST have an `enum` type.
- `throw` is a **contextual keyword**: it is permitted only in functions with `throws` or `throws<E>` in their effect set (§11.2).
- In functions with `throws<E>`, the thrown enum type MUST be exactly `E`.
- Outside a function with a `throws` effect, `throw` has no special meaning and may be used as an ordinary identifier.

### 14.2 `try` (propagate and handle)

`try` is a **contextual keyword**: in expression position, it introduces a try-expression. Elsewhere, it may be used as an ordinary identifier.

`try` is an expression with two forms:

1) Propagate:
- `try expr`

If `expr` throws, the thrown enum value is propagated out of the current function.

2) Handle:
- `try expr else fallback_expr`
- `try expr else { arms }`

**Simple handler form** (`try expr else fallback_expr`):
- If `expr` throws any error, `fallback_expr` is evaluated and becomes the result.
- The thrown value is discarded (not bound to any variable).

**Pattern-matching handler form** (`try expr else { arms }`):
- Arms use the same syntax as `match` arms (§9.6).
- The thrown enum value is the scrutinee for pattern matching.
- Arms are tried in source order; the first matching arm's expression becomes the result.

Syntax:
```p7
try expr else {
  pattern1 => expr1,
  pattern2 => expr2,
  _ => fallback_expr,
}
```

Example:
```p7
enum FileError(
  NotFound,
  PermissionDenied,
);

fn[throws<FileError>] read_file(path: string) -> string { ... }

fn safe_read(path: string) -> string {
  try read_file(path) else {
    err: FileError.NotFound => "",
    err: FileError.PermissionDenied => "[access denied]",
  }
}
```

Arm patterns follow the same rules as `match` (§9.6.1):
- **Wildcard**: `_` matches any thrown value.
- **Path patterns**: `EnumName.VariantName` matches a specific enum variant (unit variants only in v1).
- **Named binding**: `name: pattern` binds `name` to the thrown value when the arm matches.

Rules:
- If `expr` completes normally, its value is the result.
- If `expr` throws:
  - in propagate form: current function evaluation aborts and the thrown value is propagated;
  - in handle form: the `else` branch (or matching arm) value is the result.

Type rule:
- Handle form: normal result and all else arm results MUST have identical type in v1.

Exhaustiveness:
- The pattern-matching handler form MUST be exhaustive (same as `match`, §9.6.4).
- Include a wildcard arm `_ => ...` to handle all error variants.

### 14.3 Calling functions with `throws` effect (explicitness rule)

If a call may throw (i.e., the callee has `throws` or `throws<E>` in its effect set), the call MUST appear inside a `try` form. Bare calls are ERROR, even within functions that themselves have a `throws` effect.

In a function without a `throws` effect:
- only the handle form is allowed: `try call else ...`
- the propagate form is ERROR.

In a function with a `throws` or `throws<E>` effect:
- either propagate or handle form is allowed.

[[TODO]] finalize propagation compatibility rules for `throws<E>` (exact-match vs subtyping). Recommended for v1: exact match.

---

## 15. Standard conversions and checks

### 15.1 Numeric rules

#### 15.1.1 Integer overflow
`int` arithmetic overflow TRAPs for fixed-width integer ops in v1.

Prelude functions (placeholder names):
- `wrapping_add(a: int, b: int) -> int`
- `checked_add(a: int, b: int) -> ?int`

#### 15.1.2 Numeric coercions
- Implicit `int -> float` promotion may occur in arithmetic/comparison.
- Other numeric conversions require explicit conversion. [[TODO]] specify syntax.
- `float -> int` is available **only** via a checked prelude function:
  - `float_to_int_checked(x: float) -> ?int`
  - Returns `null` if `x` is NaN, infinite, or outside the `int` range.
  - Otherwise returns the truncated-to-zero integer value.

### 15.2 Nullability

#### 15.2.1 Control-flow narrowing

If `x: ?T` and `x` is a simple identifier:

```p7
if x != null { ... } else { ... }
```

Then:
- In the then-branch, `x` is treated as type `T`.
- In the else-branch, `x` is treated as `null`.

#### 15.2.2 Null-coalescing

`x ?? default_expr`:
- If `x` is non-null, yields inner `T`.
- Else yields `default_expr`.

Rule: `default_expr` MUST have type `T`.

#### 15.2.3 Force unwrap

`x!`:
- Requires `x: ?T`.
- If `x` is non-null, yields `T`.
- If `x` is `null`, evaluation TRAPs.

### 15.3 Heap handle coercions

#### 15.3.1 `box<T>` to `robox<T>` capability-weakening

In **checking/expected-type contexts**, a `box<T>` value may implicitly coerce to `robox<T>`:

- Assignment to an annotated `robox<T>` type: `let rb: robox<T> = b;` where `b: box<T>`.
- Parameter passing: `f(b)` where `f` expects `robox<T>` and `b: box<T>`.
- Return: `return b;` where the function return type is `robox<T>` and `b: box<T>`.
- Branch/join: if/else branches with expected type `robox<T>` may return `box<T>` expressions.

The reverse coercion `robox<T> -> box<T>` is **not** allowed (ERROR).

**Rationale:**

This coercion is safe because it removes capabilities (mutation) without adding any. It enables flexible API design where functions can accept read-only handles while callers with mutable handles can pass them without explicit conversion.

---

## 16. Memory/runtime model (informative)

Protosept uses a GC runtime. Semantics are defined in terms of:
- moves/copies
- non-escapable borrowed views
- boxed identity containers

Implementation may represent values on stack or heap; this is not semantically observable.

---

## 17. Host interop (v1 requirements)

### 17.1 Calling into Protosept

Host calls a named Protosept function with arguments and receives one of:
- Returned(value)
- Threw(enum_value) if a `throw` escapes
- Trapped(panic) if a TRAP occurs

[[TODO]] specify concrete embedding API.

### 17.2 Calling host functions from p7

Host may register functions callable by p7.

Interop requirements:
- `?T` maps to/from host null.
- `ref<T>` MUST NOT cross the boundary as a persistent value (may be disallowed entirely; [[TODO]] define).
- `box<T>` is the primary mechanism for passing identity/mutable objects across the boundary.
- `box<P>` (proto boxes) is the mechanism for dynamic dispatch across the boundary (§18).

### 17.3 Ownership rules

- Passing a value type follows move/copy rules.
- Passing `box<T>` or `ref<T>` passes/copies the handle/view per §6.

### 17.4 Generics and interop

Generics are compile-time only (monomorphization). Exported host entrypoints MUST be monomorphic.

Runtime polymorphism for interop is via proto boxes (`box<P>`).

---

## 18. Protos (conformance interfaces + optional dynamic dispatch)

### 18.1 Overview

A `proto` defines a structural conformance interface: a set of required methods.

A type `T` satisfies a proto `P` if `T` provides methods matching every required signature.

### 18.2 Proto categories

- **Constraint protos**: compile-time only, no runtime dispatch (`Copy`, `Send`).
- **Object protos**: compile-time conformance + runtime dynamic dispatch via `box<P>` and `ref<P>`.

User-declared `proto` are object protos in v1.

### 18.3 Proto declaration

```p7
proto Printable {
  fn print(ref self) -> unit;
}

proto Mutator {
  fn mutate(box self);
}
```

**Receiver requirements:**

Proto methods may declare receivers as defined in §11.4:
- `self: ref<Self>` (or shorthand `ref self`) – borrowed receiver
- `self: box<Self>` (or shorthand `box self`) – boxed receiver

v1 restrictions:
- Proto methods MUST NOT mention `Self` in parameter or return types beyond the receiver. [[TODO]] future extension.
- Overloads in protos: ERROR in v1 (recommended).

### 18.4 Proto handles

There is no plain runtime value of proto type `P`.

Runtime proto handles are:
- `box<P>` – owned proto handle (§18.5)
- `ref<P>` – borrowed proto handle (§18.4.1)

Constraint protos MUST NOT appear as `box<P>` or `ref<P>`.

#### 18.4.1 Borrowed proto handles: `ref<P>`

**Well-formedness:**
- `ref<P>` is well-formed only when `P` is an object proto.
- Using `ref<P>` where `P` is a constraint proto is ERROR.

**Meaning:**
- A value of type `ref<P>` is a borrowed view of some dynamic type `T` satisfying proto `P`.

**Non-escapable rule:**
- `ref<P>` follows the non-escapable rule from §7.3.

**Dereferencing:**
- `*r` where `r: ref<P>` is ERROR in v1.

**Method-call restriction:**
- `ref<P>` can call only proto methods whose receiver is `ref self`.
- Calling a proto method with a `box self` receiver on `ref<P>` is ERROR (see §18.7).


### 18.5 Converting `box<T>` to `box<P>`

Two ways:

1) Explicit cast (allowed when `T` satisfies `P`):
```p7
let p: box<Printable> = v as box<Printable>;
```

2) Implicit coercion (allowed only when `T` declares `[P]` conformance via `struct[...]` or `enum[...]`; see §18.6):
```p7
let p: box<Printable> = v;
```

Conversion does not allocate a new `T`. It reinterprets the existing handle with a dispatch table for `P`.

[[TODO]] finalize cast syntax and coercion sites.

#### 18.5.1 Converting `ref<T>` to `ref<P>` (borrowed upcast)

A `ref<T>` can be converted to `ref<P>` when `T` satisfies `P`.

**Explicit cast:**
```p7
let r: ref<SomeStruct> = ref(v);
let p: ref<Printable> = r as ref<Printable>;
```

**Implicit coercion:**
- Recommended to allow implicit `ref<T> -> ref<P>` coercions at the same sites as `box<T> -> box<P>` coercions (assignment, argument passing, return).
- Only when `T` declares `[P]` conformance via `struct[...]` or `enum[...]` (see §18.6).
- Such coercions are subject to the restriction that only `ref self` methods can be called on `ref<P>` (§18.4.1).

[[TODO]] finalize cast syntax and coercion sites.


### 18.6 Declaring proto conformances on structs and enums

A struct or enum may declare conformances in a bracket list:

```p7
struct[Printable, Copy] Vec2(
  x: float,
  y: float,
) {
  pub fn print(self: ref<Self>) -> unit { ... }
}

enum[Printable, Copy] Status(
  Pending,
  Active: int,
  Failed: (string, int),
) {
  pub fn print(self: ref<Self>) -> unit { ... }
}
```

Rules:
- Each name in `struct[...]` or `enum[...]` MUST be the name of a proto.
- The compiler MUST check structural satisfaction.
- Listing a proto MAY enable implicit behaviors described by this spec:
  - `Copy` and `Send` opt-in behavior (§6.3, §6.5).
  - Implicit `box<T> -> box<P>` coercions for object protos (§18.5).
  - Implicit `ref<T> -> ref<P>` coercions for object protos (§18.5.1).

The conformance list does not inject members; it only checks and enables implicit behaviors.

[[TODO]] decide whether duplicate conformances are ERROR (recommended: yes).

### 18.7 Dynamic dispatch

Calling a proto method on `box<P>` or `ref<P>` performs dynamic dispatch:
- The call dispatches to the implementation for the dynamic type of the underlying object.

**Receiver semantics:**

For `box<P>`:
- For methods with `ref self` receivers: the proto box handle is passed and dereferenced to obtain a `ref<T>` view of the boxed contents.
- For methods with `box self` receivers: the proto box handle itself is passed (as `box<P>`), aliasing the original box. The method receives a boxed handle, which is Copy-treated; multiple calls do not move the box.

For `ref<P>`:
- For methods with `ref self` receivers: the borrowed proto handle is passed directly as a `ref<T>` view to the underlying object.
- For methods with `box self` receivers: calling such methods on `ref<P>` is ERROR (see §18.4.1).

Example:
```p7
proto Mutator {
  fn mutate(box self);
}

struct Counter(count: int) {
  pub fn mutate(box self) {
    self.count = self.count + 1;
  }
}

let b: box<Mutator> = box(Counter(0)) as box<Mutator>;
b.mutate();  // dispatches to Counter.mutate; box handle is Copy-treated
b.mutate();  // ok: can call again
```

### 18.8 Downcasting / type tests
[[TODO]] runtime type tests and downcasts for proto boxes.

### 18.9 Nullability
`?box<P>` is a nullable proto handle; `box<P>` is non-null.

---

## 19. Attributes (compile-time metadata)

Attributes are typed metadata values attached to declarations.

Properties:
- typed (schema is a `struct`)
- compile-time only
- inert by default; semantics only when explicitly specified by this spec or an extension
- preserved in compiled artifact in a host-visible form

### 19.1 Attachment sites (v1)
An attribute list may appear immediately before a top-level:
- `fn`, `struct`, `enum`, `type`

[[TODO]] attributes for `proto`, fields, variants, params, locals.

### 19.2 Syntax

`@AttrName(...)` where `AttrName` resolves to a `struct` name. Parentheses required: `@AttrName()`.

Multiple attributes are written by repetition:
```p7
@doc("Entrypoint")
@export(name = "main")
fn main() -> unit { ... }
```

*   Note: The `@` symbol is exclusively for attributes. Heap boxing uses the `box` keyword or the `^` sigil to avoid ambiguity.

### 19.3 Values are typed struct constructors
Attribute arguments follow struct construction rules (required fields provided, defaults allowed, names must match).

### 19.4 Const restrictions (v1)

Attribute constructor arguments MUST be compile-time constants. Permitted field types:
- primitives, `string`, enums
- `?T` where `T` permitted
- `array<T>` where `T` permitted
- user structs whose fields are recursively permitted

### 19.5 Ordering and duplicates
Attributes are an ordered list. Duplicates allowed; order preserved.

### 19.6 Compiled representation (normative)
Compiled artifact MUST preserve, for each attributed declaration:
- kind (`fn`/`struct`/`enum`)
- name (including module qualification when modules exist)
- ordered list of attribute instances

### 19.7 Errors
ERROR if:
- attribute name does not resolve to a `struct`
- unknown named field provided
- required field omitted
- non-constant value provided
- field type not permitted

---

## 20. Generics (compile-time only)

### 20.1 Overview

Generics are monomorphized:
- The compiler generates specialized code for each concrete instantiation.
- No runtime representation of type parameters.

### 20.2 Generic functions

```p7
fn identity<T>(x: T) -> T { return x; }
```

#### 20.2.1 Type arguments at call sites

A reference to a generic function in a call position MAY be specialized with an explicit type argument list:

- `name<T1, T2, ...>(args...)`

Rules:
- When type arguments are provided explicitly:
  - The number of type arguments MUST exactly match the function's type parameter list; otherwise ERROR.
  - Each provided type argument MUST be a well-formed type.
  
- When type arguments are omitted, the compiler attempts to infer them (§0.1):
  - Inference uses argument types and the expected type (if available).
  - If a unique instantiation can be determined, the call is accepted.
  - If multiple instantiations are possible or none can be determined, it is an ERROR. The compiler MUST report: "cannot infer type arguments for `name`; explicit type arguments required."

Examples:
```p7
// Explicit type arguments (always allowed)
identity<int>(1);
identity<string>("hi");

// Inference from argument types
let x = identity(42);        // OK: inferred as identity<int>(42)
let s = identity("hello");   // OK: inferred as identity<string>("hello")

// Inference from expected type
fn get_default<T>() -> T { ... }
let n: int = get_default();  // OK: inferred as get_default<int>()

// Ambiguous case (ERROR)
fn ambiguous<T>() -> T { ... }
let y = ambiguous();         // ERROR: cannot infer T (no arguments, no expected type)
```

### 20.3 Generic structs

```p7
struct Pair<T, U>(first: T, second: U);
```

#### 20.3.1 Explicit type arguments in type positions (v1)

In a type position, type arguments use the existing type syntax:
- `Pair<int, int>`
- `array<Pair<int, string>>`
- `box<Pair<float, float>>`

#### 20.3.2 Type arguments at construction sites

Construction of a generic struct uses the struct's name, optionally specialized with an explicit type argument list:

- `Name<T1, T2, ...>(args...)`
- `Name(args...)` (if type arguments can be inferred)

Rules:
- When type arguments are provided explicitly:
  - The number of type arguments MUST exactly match the struct's type parameter list; otherwise ERROR.
  
- When type arguments are omitted, the compiler attempts to infer them (§0.1):
  - Inference uses field argument types and the expected type (if available).
  - If a unique instantiation can be determined, the construction is accepted.
  - If multiple instantiations are possible or none can be determined, it is an ERROR. The compiler MUST report: "cannot infer type arguments for `Name`; explicit type arguments required."

Examples:
```p7
// Explicit type arguments (always allowed)
let p = Pair<int, int>(1, 1);
let q = Pair<string, int>("a", 2);

// Inference from field argument types
let r = Pair(42, 3.14);      // OK: inferred as Pair<int, float>(42, 3.14)
let s = Pair("x", "y");      // OK: inferred as Pair<string, string>("x", "y")

// Inference from expected type
fn needs_pair() -> Pair<int, string> {
  return Pair(100, "ok");    // OK: inferred from return type
}
```

### 20.4 Generic enums

Generic enums support type parameters:
```p7
enum Name<T, U>(
  Variant1,
  Variant2: T,
  Variant3: (T, U),
);
```

#### Type arguments in type positions

In a type position, type arguments use the existing type syntax:
- `Option<int>`
- `Result<string, Error>`
- `array<Option<int>>`

#### Type arguments at construction sites

Construction of a generic enum variant uses the enum's name, optionally specialized with an explicit type argument list:

- `Name<T1, T2, ...>.VariantName` for unit variants
- `Name<T1, T2, ...>.VariantName(args...)` for payload variants
- `Name.VariantName(args...)` (if type arguments can be inferred from payload)
- `Name.VariantName` (only if type arguments can be inferred from context)

Rules:
- When type arguments are provided explicitly:
  - The number of type arguments MUST exactly match the enum's type parameter list; otherwise ERROR.
  
- When type arguments are omitted, the compiler attempts to infer them (§0.1):
  - For payload variants, inference uses payload argument types and the expected type (if available).
  - For unit variants (no payload), inference requires an expected type; otherwise ERROR.
  - If a unique instantiation can be determined, the construction is accepted.
  - If multiple instantiations are possible or none can be determined, it is an ERROR. The compiler MUST report: "cannot infer type arguments for `Name`; explicit type arguments required."

**Important**: Unit variants like `Option.None` or `Result.Err` without payload arguments MUST have type arguments determined by context (e.g., explicit type annotation, return type, parameter type). Without context, they are ambiguous and produce an ERROR.

#### Example: `Option<T>`

```p7
enum Option<T>(
  None,
  Some: T,
);

fn example() -> unit {
  // Explicit type arguments (always allowed)
  let x = Option<int>.Some(42);
  let y = Option<string>.None;
  
  // Inference from payload argument type
  let a = Option.Some(42);       // OK: inferred as Option<int>.Some(42)
  let b = Option.Some("hi");     // OK: inferred as Option<string>.Some("hi")
  
  // Unit variant requires context
  let c: Option<int> = Option.None;  // OK: inferred from annotation as Option<int>.None
  // let d = Option.None;         // ERROR: cannot infer T (no payload, no context)
  
  // Inference from expected type (return type)
  fn get_some() -> Option<float> {
    return Option.Some(3.14);    // OK: inferred from return type
  }
}
```

#### Example: `Result<T, E>`

```p7
enum Result<T, E>(
  Ok: T,
  Err: E,
);

fn example() -> unit {
  // Explicit type arguments (always allowed)
  let success = Result<int, string>.Ok(100);
  let failure = Result<int, string>.Err("network error");
  
  // Inference from payload argument types
  let r1 = Result.Ok(42);         // Partial inference: T=int, but E is unknown
                                   // ERROR: cannot fully infer Result<T, E>
  
  // Inference from expected type
  fn process() -> Result<int, string> {
    return Result.Ok(100);         // OK: inferred as Result<int, string>.Ok(100)
    // return Result.Err("fail");  // OK: inferred as Result<int, string>.Err("fail")
  }
  
  // Mixed: one type from payload, one from context
  let r2: Result<int, string> = Result.Ok(100);  // OK: T from payload, E from context
}
```

Note: Without pattern matching in v1, generic enums are primarily useful for:
- Type-safe value construction and passing
- API design and future extensibility
- Host interop that may inspect enum variants externally


### 20.5 Bounds

Type parameter bounds use proto constraints:
```p7
fn print_boxed<T: Printable>(value: box<T>) -> unit { value.print(); }
```

v1: only a single proto bound per type parameter.

Example with `Copy`:
```p7
fn duplicate<T: Copy>(x: T) -> Pair<T, T> {
  return Pair<T, T>(x, copy(x));
}
```

---

## 21. Extension: Fibers (cooperative coroutines)

Status: optional extension.

### 21.1 Availability
When disabled, functions with the `suspend` effect and `yield` are ERROR.

[[TODO]] define how to enable (flag/import/host config).

### 21.2 Functions with `suspend` effect

A function with the `suspend` effect (declared as `fn[suspend]` or `fn[suspend, ...]`) may suspend via `yield;`.

Example:
```p7
fn[suspend] fiber_task() {
  yield;
  // ... continues after resume
}

fn[throws, suspend] async_operation() -> int {
  yield;
  if error_condition { throw SomeError.Failed; }
  return 42;
}
```

Borrow restriction (v1):
- In a function with the `suspend` effect, use of `ref<...>` is forbidden:
  - parameters of type `ref<T>` are ERROR
  - locals of type `ref<T>` are ERROR
  - `ref(x)` expression is ERROR
Rationale: avoids views living across suspension points without lifetime tracking.

Direct calling restriction (recommended):
- Functions with the `suspend` effect may be called directly only from within other functions with the `suspend` effect. [[TODO]] finalize.

### 21.3 `yield;`

`yield;` is a **contextual keyword**: in statement position within a function with the `suspend` effect, it suspends the current fiber. Elsewhere, it may be used as an ordinary identifier.

- `yield;` suspends the current fiber.
- On resume, execution continues after the `yield;`.

### 21.4 Host interop (fibers)

Host must support:
- starting a fiber from a function with the `suspend` effect
- resuming a fiber until it yields/returns/throws

Outcome per resume:
- Yielded
- Returned(value)
- Threw(enum_value)

### 21.5 `spawn` (start a new fiber)

Form:
```p7
spawn f(args...);
```

Rules:
- `f` MUST refer to a function with the `suspend` effect.
- `spawn` is a statement returning `unit` in v1.

Semantics:
- Requests creation of a new fiber; host decides when/if it runs.
- Host hook `on_fiber_spawn(handle, info)` is invoked. [[TODO]] define info fields.

---

## 22. Extension: Threading (actor-like isolation)

Status: optional extension.

### 22.1 Availability
When disabled, `spawn_thread` is ERROR.

[[TODO]] define enabling mechanism.

### 22.2 Send-gated transfer
Arguments to `spawn_thread` MUST satisfy `Send` (§6.5). This prevents shared mutable state across threads.

### 22.3 `spawn_thread`

Form:
```p7
spawn_thread f(args...);
```

Rules:
- `f` is a (non-suspend) function.
- All argument types MUST satisfy `Send`.
- `spawn_thread` is a statement returning `unit`.

Semantics:
- Requests creation of a new thread execution; host controls scheduling.
- Host hook `on_thread_spawn(handle, info)` is invoked.

### 22.4 Thread completion outcomes

Thread completes with:
- Returned(value) where return type satisfies `Send` (or is `unit`) — otherwise ERROR at compile time.
- Threw(enum_value) where enum type satisfies `Send` — otherwise ERROR at compile time.
- Trapped(panic) — terminates the thread; does not affect other threads.

### 22.5 Interaction with fibers
If both extensions enabled:
- Fibers are pinned to a thread (no migration).
- `spawn` is thread-local; its arguments need not satisfy `Send`.
- A TRAP in any fiber terminates the entire thread.

[[TODO]] message passing/channels.

---

## 23. Open items / TODO list (curated)

1) String concatenation spelling, slicing APIs
2) Boxed array mutation API surface and semantics (§3.3.3)
3) Coercion sites and cast spelling for `box<T> -> box<P>` (§18.5)
4) Enablement mechanisms for extensions (§21, §22)
5) Host ABI: concrete API surfaces for calling, fibers, threads (§17, §21.4, §22)
6) Specify prelude location/definition of `box<T>.new` intrinsic method (§7.4)
7) Specify representation/ABI attribute like `@repr(transparent)` for structs (especially for newtype/FFI)

---
End.
