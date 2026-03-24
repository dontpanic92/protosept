# Protosept Language Specification (Draft 1.1)

Status: Draft (v1 target)

> **Protosept** is the public name of the language.
> The short name **p7** may still appear in tooling, file extensions,
> and internal identifiers. Unless otherwise stated, "Protosept" and "p7" refer to the same language.

## Design Goals

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
- **Slot**: a storage location introduced by `let`, `let mut`, or a parameter.
- **Addressable location** (v1): a `let`-introduced slot, a parameter slot, or a field/sub-location of an addressable base where the language provides addressability (see §7.1, §7.2, §11.4). Note: `let mut` slots are NOT addressable locations in v1.
- **Mutable place**: a syntactic expression that may appear on the left side of an assignment statement or as the receiver of a `ref mut self` method call (see §10.2, §11.3.2, §11.4). Mutable places are ephemeral; they are not storable values.
- **Binding mutability**: whether a named slot may be reassigned. Controlled by `let` vs `let mut`. A `let mut` binding may be rebound to a new value; a `let` binding may not.
- **Interior mutability**: whether the value behind a handle may be mutated in place. Controlled by the handle type: `box<T>` permits interior mutation; `robox<T>` does not. `let mut` has no effect on interior mutability.
- **Structural-copyable**: types for which `structural_copy(x)` is well-typed (§6.2). This is a structural property determined by the type's structure.
- **Copy type**: a type `T` such that `T: Copy` (§6.3). Types satisfying the `Copy` proto enable implicit copying at value-flow sites.
- **Materialized temporary slot (v1)**: an implicit immutable `let` slot created by the compiler to extend the lifetime of a temporary value, enabling it to be borrowed. Used in narrowly-scoped contexts; in v1, this is currently only used for receiver temporary materialization (§11.3.1).


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

### 1.1 Packages, modules, imports, visibility

#### 1.1.1 Packages

A **package** is the unit of compilation and dependency distribution.

- Package names are chosen by the host or tooling.
- The compiler accepts a package name and a set of source files (modules).
- A package may depend on other packages; dependencies are provided by host/tooling.

#### 1.1.2 Modules

A **module** is a single source file.

- Each source file in a package is one module.
- A module has a **module path** derived from its file path within the package (host/tooling-defined mapping).
  - Recommended mapping: `/` becomes `.`.
  - Example: `src/util/string.p7` → `mypackage.src.util.string`.

##### 1.1.2.1 Builtin package

The **builtin package** is a compiler-bundled package that is automatically loaded before user code.

- The builtin package is **always available**, even in `nostd` mode (when an optional standard library is not loaded).
- It declares fundamental types using `@builtin()` structs (§12.6), providing discoverable method signatures for IDE navigation (e.g., F12 "Go to Definition").
- Builtin types include `string` and other compiler-defined nominal types.
- Methods in the builtin package are typically marked `@intrinsic()` (§19.8.2), meaning they have no runtime implementation source; instead, the compiler lowers calls to these methods directly to intrinsic operations during codegen.

**Distinction from standard library:**
- **Builtin package**: Compiler-bundled, always loaded, contains fundamental types and intrinsics. Available in all compilation modes.
- **Standard library (stdlib)**: Optional, user-loadable library with utility functions, data structures, etc. May be excluded in `nostd` mode.

The builtin package provides the canonical declarations for types like `string`, allowing method calls such as `s.len_bytes()` to resolve through normal method resolution, while the compiler generates intrinsic code at compile time.

#### 1.1.3 Absolute module paths

An **absolute module path** begins with a package name and uses `.` as a separator.

Examples:
- `std.collections.list`
- `myapp.services.auth`

Qualified names may be used in any name position (types, expressions, etc.), except for leading-`.` relative paths which are restricted to `import` (§1.1.5).

#### 1.1.4 Import statements

`import` brings a module into scope.

Syntax:
```p7
import <module-path>;
import <module-path> as <name>;
```

`import P;` binds the last segment of `P`.  
`import P as N;` binds `N`.

After import, the bound name refers to the imported module; its public (`pub`) declarations are available via that name.

> **Resolution**: The compiler is filesystem-agnostic. A host-provided resolver supplies module sources by path; the only bundled module is `builtin`.
> **Visibility**: All modules are public by default.

Example:
```p7
import std.collections.list;
import myapp.services.auth as Auth;

list.new_list();
Auth.login();
```

#### 1.1.5 Relative module paths (import-only)

A relative module path begins with `.` and is permitted **only** in `import`.

- `.foo` refers to a sibling module `foo`.
- `.sub.bar` refers to module `bar` in subdirectory `sub`.

Example:
```p7
// In module `myapp.services.auth`
import .helpers;          // `myapp.services.helpers`
import .sub.utilities;    // `myapp.services.sub.utilities`
```

#### 1.1.6 Package-root relative imports (import-only)

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

#### 1.1.7 Visibility

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

### 1.2 Name resolution (v1)

Name resolution determines which declaration a given identifier refers to. Protosept uses two distinct namespaces and employs lexical scoping with module-level imports.

#### 1.2.1 Two namespaces

Protosept maintains two separate namespaces:

1. **Type namespace**: For types (structs, enums, protos, type parameters).
2. **Value namespace**: For values (functions, variables, enum variants, struct/enum constructors).

A name may simultaneously exist in both namespaces without conflict:

```p7
struct Point(x: int, y: int);
let Point = 5;  // ok: `Point` is a type AND a value binding (though not idiomatic)
```

In type position (e.g., after `:`), only the type namespace is consulted. In expression position, only the value namespace is consulted.

#### 1.2.2 Scopes

Scopes form a nested hierarchy:

1. **Module scope**: Top-level declarations in the current module.
2. **Import scope**: Names introduced by `import` statements (effectively part of module scope).
3. **Block scopes**: Introduced by `{ }`, `fn` bodies, `loop`, etc.

Name lookup proceeds from the innermost scope outward until a binding is found or the search fails.

#### 1.2.3 Imports introduce a module binding

An `import` statement introduces a **value binding** for the imported module:

```p7
import std.collections.list;
```

This binds `list` as a **module value** in the importing module's scope. Module members are accessed via qualification: `list.new_list()`.

- The import binding exists in the **value namespace**.
- A module binding is a first-class value that supports member access via `.` for its public declarations.

#### 1.2.4 Constructors are values (v1)

Structs and enums introduce bindings in both namespaces:

- **Type namespace**: The type name (e.g., `Point` as a type).
- **Value namespace**: The constructor (e.g., `Point(...)` as a function-like value).

Example:
```p7
struct Point(x: int, y: int);

let p: Point = Point(1, 2);
//     ^^^^^   ^^^^^^^^^^^^
//     type    constructor (value)
```

Enum variants are **values only** (they exist only in the value namespace, not as types):

```p7
enum Status(
  Pending,
  Active: int,
);

let s1 = Status.Pending;        // value
let s2 = Status.Active(42);     // value (constructor)
let t: Status = s1;             // `Status` is the type
```

#### 1.2.5 Qualified names

A **qualified name** uses `.` to access a member of a module, type, or value:

- **Module qualification**: `moduleName.member`
  ```p7
  import myapp.util.helpers;
  helpers.do_something();
  ```

- **Type-associated members** (e.g., enum variants, associated functions):
  ```p7
  Status.Pending
  Status.Active(42)
  ```

Qualified names are resolved left-to-right: the left-hand side determines the context, and the right-hand side is resolved within that context.

#### 1.2.6 Resolution in type position

In type position (e.g., `let x: T`, `fn f() -> T`, field types), the compiler resolves names as follows:

1. **Unqualified name** `T`:
   - Search type namespace in current scope (including type parameters).
   - If not found, search module scope for type declarations.
   - If not found, ERROR: "unresolved type `T`".

2. **Qualified name** `M.T`:
   - Resolve `M` in the value namespace (must be a module).
   - Search `M`'s type namespace for `T`.
   - If not found or not visible, ERROR.

Example:
```p7
import std.collections.list;

fn process(items: list.List<int>) -> int {
  //               ^^^^^^^^^^^^^^
  //               `list` is a module, `List` is a type in that module
  ...
}
```

#### 1.2.7 Resolution in expression position

In expression position, the compiler resolves names in the value namespace.

##### 1.2.7.1 Call-head resolution (v1)

When an identifier appears in call position `f(...)`, the compiler:

1. Looks up `f` in the value namespace.
2. If `f` is a constructor (struct or enum), it is invoked as a constructor.
3. If `f` is a function, it is invoked as a function call.
4. If `f` is not callable, ERROR: "`f` is not callable".

Qualified calls follow the same pattern:

```p7
import myapp.util;

util.helper();          // `util` is a module, `helper` is a function
Status.Active(42);      // `Status` is a type (constructor), `Active` is a variant constructor
```

##### 1.2.7.2 Dotted resolution: module qualification vs member access

The `.` operator is context-sensitive:

- **Module qualification**: When the left-hand side is a module binding, `.` accesses module members.
  ```p7
  import myapp.services;
  services.auth();  // `services` is a module
  ```

- **Member access**: When the left-hand side is a value, `.` accesses struct fields or invokes methods.
  ```p7
  let p = Point(1, 2);
  p.x               // field access
  p.distance()      // method call
  ```

The compiler determines which interpretation applies based on the type of the left-hand side.

#### 1.2.8 Enum variant qualification (v1)

Enum variants are always accessed via the enum type name:

```p7
enum Status(Pending, Active: int);

let s = Status.Pending;      // ok
let s = Pending;             // ERROR: unresolved name `Pending`
```

Rationale: This eliminates ambiguity when multiple enums define variants with the same name.

#### 1.2.9 Generic parameter naming restriction (v1)

Type parameters must not shadow type names from outer scopes:

```p7
struct Outer<T>(value: T);

struct Inner<T>(         // ok: `T` is a new parameter
  outer: Outer<T>,       // refers to `Inner`'s `T`
);

fn process<T>(x: T) {
  struct Local<T>(v: T); // ERROR: type parameter `T` shadows outer `T`
}
```

**Rule**: A type parameter name must not conflict with:
- A type parameter in an enclosing scope (function or type declaration).
- A type name declared in the module scope.

This restriction prevents confusion about which `T` a reference refers to. If shadowing is needed, use a different name (e.g., `U`, `V`).

---

## 2. Lexical structure

### 2.1 Identifiers
Identifiers start with `_` or a letter and continue with letters, digits, or `_`.

### 2.2 Keywords and identifiers

**Reserved keywords** (minimal set):  
`fn`, `struct`, `enum`, `proto`, `let`, `mut`, `pub`, `return`, `if`, `else`, `loop`, `break`, `continue`, `for`, `in`, `import`, `as`

`true` and `false` are **keywords** (boolean literals).  
`null` is a keyword (null literal).

**Predeclared type constructors / intrinsics** (not reserved; contextual by syntactic position):  
`ref`, `box`, `robox`

These identifiers have special meaning only in specific syntactic positions:
- **Type position:** `ref<T>`, `box<T>`, `robox<T>` denote reference, boxed, and read-only boxed types.
- **Expression position:** `ref(expr)` and `box(expr)` construct reference and boxed values. Note: `robox` has no expression-position form; `robox<T>` values are obtained via type ascription or coercion from `box<T>`.
- **Method receiver position:** `ref self` desugars to `self: ref<Self>`, `box self` desugars to `self: box<Self>`, and `ref mut self` denotes an ephemeral mutable-borrowed receiver (see §11.4). Note: `robox self` is not a valid receiver form.
- In all other positions, `ref`, `box`, and `robox` are ordinary identifiers and may be used as variable names, parameter names, etc.

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

To facilitate rapid authoring without sacrificing the auditability of the final code, the compiler accepts specific symbols (sigils) as synonyms for `ref` and `box`.

**Canonicalization Rule:**
While the compiler accepts these sigils, standard formatters and linters are encouraged to replace them with their word-form equivalents (`ref`, `box`) in stored source files to maximize readability for reviewers.

| Sigil | Word-form Equivalent | Meaning | Usage (Type) | Usage (Expr) |
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
- Function types: `fn(T1, T2, ...) -> R`, `fn[effects](T1, T2, ...) -> R` (§3.9)
- User-defined: `struct Name(...)`, `enum Name(...)`, `proto Name { ... }`
- Compile-time generics: `T`, `array<T>`, `box<T>`, `robox<T>`, etc. (§20)

---

### 3.1 Primitive types

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
  - Structural-copyable and satisfies `Copy`.
  - Allowed operations (v1): `==` and `!=`.
  - Other operations (arithmetic, dereferencing) are TODO and expected only under FFI/unsafe extension.

---

### 3.2 `string`

- `string` is a built-in **immutable value type** containing UTF-8 text.
- `string` is a **builtin nominal type** with compiler-defined representation (§1.1.2.1).
- Its canonical declaration and method signatures are declared in the builtin package as an `@builtin()` struct (§12.6).
- Iteration unit is `char`.

Minimum v1 operations (declared as intrinsic methods in the builtin package):
- `len_bytes(self: ref<string>) -> int` — Returns the byte length of the string (UTF-8 encoded).
- `len_chars(self: ref<string>) -> int` — Returns the character count (number of Unicode scalar values). [[TODO]]
- `get_char(self: ref<string>, i: int) -> ?char` — Returns the character at index `i` (0-based; out of bounds → `null`). [[TODO]]

**Method call syntax:**
```p7
let s = "hello";
let byte_len = s.len_bytes();  // 5
```

Indexing policy:
- No `s[i]` syntax for strings in v1.

String literal syntax and escapes are defined in §4.3.

[[TODO]] concatenation spelling, slicing APIs.

---

### 3.3 `array<T>`

- `array<T>` is a built-in **immutable value type**.
- In-place mutation of a value array is not supported in v1.
- Shared mutation/identity is provided via `box<array<T>>` with mutation APIs (§7.4, §3.3.3).

#### 3.3.1 Array literals

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

#### 3.3.2 Array indexing

Two indexing forms:

1) **Trap indexing**:
- `a[i]` reads element at index `i` (0-based).
- If `i` is negative or out of bounds, evaluation TRAPs.

2) **Checked indexing**:
- `a.get(i)` returns `?T`.
- If `i` is negative or out of bounds, returns `null`.

**Boxed-array index as mutable place:**
- When `a: box<array<T>>`, the expression `a[i]` is a **mutable place** in addition to being a read expression.
- As a mutable place, `a[i]` may appear on the left of an assignment or as the receiver of a `ref mut self` method call (see §10.2, §11.3.2).
- `a[i]` is NOT a mutable place when `a: array<T>` (value arrays are immutable) or when `a: robox<array<T>>` (read-only box).

[[TODO]] define full array API surface (`len`, `get`, etc.) and whether `get` is syntax sugar for a prelude function.

#### 3.3.3 Boxed array mutation

Mutation of an array requires boxing:
- `box<array<T>>` represents a mutable, identity-bearing container.

**Structural mutation** (adding/removing elements) is via library operations on the boxed array, such as `push`, `pop`, `set`, and `insert`.

**Element mutation in place** is via mutable place assignment or `ref mut self` method calls on elements:
- `xs[i] = new_val;` — replace element at index `i`
- `xs[i].field = 10;` — assign to a field of the element at index `i` (requires element type to be a struct with a visible field)
- `xs[i].method()` where `method` has a `ref mut self` receiver — call a mutable method on element `i` in place

See §10.2 for assignment rules and §11.3.2 for `ref mut self` auto-borrow sugar.

**Restriction:** When `xs[i]` is used as a mutable place within an expression, the same expression MUST NOT also perform structural mutation of `xs` (push, pop, insert, remove, or replacement of `xs` itself). This local exclusivity rule prevents element references from being invalidated within a single expression. See §10.2.1.

[[TODO]] specify full boxed-array API (e.g. `push`, `set`, `pop`, `insert`, `remove`) and their signatures.

---

### 3.4 Tuple types

Tuples are built-in **immutable value types** that group multiple values of potentially different types.

#### 3.4.1 Tuple type syntax

A tuple type is written as `(T1, T2, ..., Tn)` where `n >= 2`.

Examples:
- `(int, string)` — a 2-tuple (pair) of an `int` and a `string`
- `(float, float, float)` — a 3-tuple of three `float` values
- `(int, (string, bool))` — nested tuples are allowed

Special cases:
- `()` is the **unit type** (not a tuple), with a single value `()`.
- `(T)` is **not** a tuple type; it is interpreted as a parenthesized type expression (i.e., just `T`).

#### 3.4.2 Tuple literals

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

#### 3.4.3 Element access

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

#### 3.4.4 Structural rules

- Tuple types satisfy `Copy` iff all component types satisfy `Copy`.
- Tuple types are **Send** when all component types are Send.
- Tuple elements cannot be mutated in-place (tuples are immutable value types).

---

### 3.5 Nullable types: `?T`

- `?T` is either `null` or a non-null `T`.
- `null` is assignable only to `?T`.
- Unwrapping and narrowing rules are in §15.2.

---

### 3.6 Borrowed view types: `ref<T>`

`ref<T>` is a **read-only view** of an existing addressable location that holds a `T` (§7).

- `ref<T>` values satisfy `Copy` (copying a `ref<T>` copies the view/handle; it does not copy the underlying `T`).
- `ref<T>` values are **non-escapable** (§7.3).

`ref<?T>` is permitted and means a view of a nullable location.

---

### 3.7 Owned heap handle types: `box<T>`

`box<T>` is an **owned heap-allocated identity container** holding a `T`.

- `box<T>` values can escape (stored, returned, captured, interop).
- `box<T>` satisfies `Copy`: copying a box copies the handle; all copies alias the same boxed cell.
- Mutation of boxed contents is visible through all aliases.

---

### 3.8 Read-only heap handle types: `robox<T>`

`robox<T>` is a **read-only heap-allocated identity container** holding a `T`.

- `robox<T>` values can escape (stored, returned, captured, interop).
- `robox<T>` satisfies `Copy`: copying a robox copies the handle; all copies alias the same boxed cell.
- `robox<T>` **forbids mutation** through the handle:
  - `*rb = ...` is ERROR when `rb: robox<T>`.
  - `rb.field = ...` is ERROR when `rb: robox<S>`.
- `robox<T>` supports borrowing boxed contents with `ref(*rb)`.
- Dereferencing `*rb` as a value is allowed only when `T: Copy`; otherwise ERROR (mirroring the `box<T>` rule).
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

### 3.9 Function types

A function type describes a callable value — a closure (§9.7) or, in the future, a function reference.

**Syntax:**
```
fn(T1, T2, ...) -> R
fn[effect1, effect2, ...](T1, T2, ...) -> R
```

- Parameter types are listed positionally; parameter names are not part of the function type.
- `-> R` specifies the return type. If omitted, the return type is `unit`.
- The effect set follows the same rules as function declarations (§11.2): effects are a **set**, order is not semantically significant, and duplicates are ERROR.

A function type with no effect list (or an empty effect list) denotes a **pure** callable — it may not throw, suspend, or perform any declared effect.

**Examples:**
```p7
fn(int) -> int                          // pure: int → int
fn(string, int) -> bool                 // pure: (string, int) → bool
fn[throws](string) -> string            // may throw any enum
fn[throws<FileError>](string) -> string // may throw FileError
fn[suspend]() -> unit                   // may suspend
fn[throws, suspend]() -> int            // may throw and suspend
fn() -> unit                            // pure: no params, returns unit
```

**Effect compatibility (assignability):**

A value of a function type with *fewer* effects is assignable to a function type with *more* effects (effect widening), because a callable that never performs an effect trivially satisfies a contract that *permits* the effect:

| Value type | Target type | Allowed? |
|---|---|---|
| `fn(T) -> R` (pure) | `fn[throws](T) -> R` | YES (widening) |
| `fn[throws<E>](T) -> R` | `fn[throws](T) -> R` | YES (typed → untyped) |
| `fn[throws](T) -> R` | `fn(T) -> R` | ERROR (narrowing) |
| `fn[throws](T) -> R` | `fn[throws<E>](T) -> R` | ERROR (cannot guarantee only `E`) |

This mirrors the propagation compatibility rules for typed throws (§14.4) and follows from the principle that *fewer effects ⊆ more effects*.

**Value-flow rules:**

Function-typed values (closures) follow the standard value-flow rule (§6.1):
- A closure is `Copy` iff all its captured values satisfy `Copy`.
- A closure is structural-copyable iff all its captured values are structural-copyable.
- A closure satisfies `Send` iff all its captured values satisfy `Send`.

---

## 4. Values and literals

### 4.1 Integer literals
Decimal digits with optional `_`: `0`, `42`, `1_000_000`

### 4.2 Float literals
Decimal with `.` and optional `_`: `1.0`, `3.1415`, `1_000.5`

**Exponent notation:**
Float literals may include an exponent suffix using `e` or `E`, followed by an optional sign (`+` or `-`) and one or more decimal digits.

Syntax: `<mantissa>e[+|-]<exponent>` or `<mantissa>E[+|-]<exponent>`

- The mantissa MUST contain a decimal point (i.e., `1e10` is ERROR; use `1.0e10`).
- The exponent digits may include `_` separators: `1.0e1_000`.
- The exponent represents a power of 10: `1.5e3` equals `1500.0`.

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

#### 4.3.1 Interpolated string literals

An interpolated string literal is written with a leading `f` prefix and a string literal:

- Syntax: `f" ... "`
- The `f` MUST appear immediately before the opening quote (no whitespace). Otherwise it is parsed as an identifier followed by a string literal.

Interpolated strings contain zero or more **interpolation holes** of the form `{ expr }`, where `expr` is a normal expression.

Examples:
```p7
let name = "Ada";
let n = 3;
let s = f"hello {name}, n={n}";
```

**Escaping `{` and `}`:**
- `{{` inside `f"..."` produces a literal `{`.
- `}}` inside `f"..."` produces a literal `}`.
- A single `}` that does not close an interpolation hole is ERROR.

All normal string escape sequences from §4.3 apply to the literal text segments.

**Parsing rule (balanced braces):**
- The body of `{ expr }` is parsed as an expression and may contain nested `{ ... }` braces (e.g., block expressions) as long as braces are balanced.
- The interpolation hole ends at the `}` that matches its opening `{` (brace nesting depth returns to 0).
- Unterminated holes are ERROR.

**Typing rule (no implicit conversion):**
For each interpolation hole expression `ei` with type `Ti`:
- `Ti` MUST satisfy `Display` (§6.4.2). Otherwise it is a compile-time ERROR.
- The `Display.display(ref self) -> string` method is used to obtain the textual representation of the hole value.
- The resulting interpolated string expression has type `string`.

This rule does **not** introduce implicit conversions between types in general expression typing. In particular:
- `let s: string = 123;` is still ERROR.
- `let s: string = f"{123}";` is OK because interpolation requires `int: Display`, not because `int` converts to `string`.

**Evaluation order and semantics:**
- Literal segments and hole expressions are evaluated left-to-right.
- The final value is the concatenation of:
  1) each literal segment (as `string`), and
  2) the result of `Display.display(...)` for each hole expression, in order.

**Lowering (desugaring, informative):**
An implementation MAY lower:
```p7
f"A{e1}B{e2}C"
```
to an equivalent sequence that:
- evaluates `e1` and `e2` once each (left-to-right),
- calls `Display.display` on each value (via normal method-call rules, including receiver temporary materialization where applicable), and
- concatenates the pieces to produce a `string`.

The concatenation mechanism is implementation-defined (e.g., repeated concatenation or a builder), but MUST preserve the observable semantics above.

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

### 5.1 `let` and `let mut` bindings (slots)

`let pattern = expr;` introduces one or more immutable slots by matching `expr` against `pattern`.

- `let` slots are single-assignment.
- Assigning to a `let` slot (e.g., `x = expr`) is ERROR.
- When `pattern` is a plain identifier `x`, this is the common form `let x = expr;` and binds the whole value to `x`.
- `pattern` MUST be an irrefutable pattern for the static type of `expr` (see §9.6.1.2); otherwise ERROR.

`let mut pattern = expr;` introduces one or more mutable slots (v1).

- `let mut` slots can be reassigned: `x = new_expr;` where `new_expr` has the same type as the slot.
- `let mut` slots are mutable but NOT addressable (see §0); borrowing via `ref(x)` where `x` is a `let mut` slot is ERROR.
- `pattern` MUST be an irrefutable pattern for the static type of `expr` (see §9.6.1.2); otherwise ERROR.

**Binding mutability vs. interior mutability:**
`let mut` expresses **binding mutability** only: the slot may be reassigned to a new value. It does NOT imply interior mutability of the bound value. Interior mutability (mutation of a value's fields or contained state) is a property of the value's type, not of the binding. Specifically:
- `let p = box(Point(1, 2));` — `p` cannot be rebound, but `p.x = 3` is valid because `p: box<Point>` provides interior mutability.
- `let mut p = box(Point(1, 2));` — `p` can be rebound to another box (e.g., `p = box(Point(4, 5))`), but whether `p.x = 3` is valid still depends on `p: box<Point>`, not on `let mut`.
- `let mut n = 0;` — `n` can be reassigned (e.g., `n = 1`), but `n: int` has no interior structure to mutate.

Values bound by a destructuring pattern flow into their bindings according to the standard value-flow rule (§6.1): `Copy` types are copied; other types are moved. The source expression is consumed exactly once.

Destructuring assignment to existing places is not supported in v1; deconstruction is only allowed at binding sites (`let`, `let mut`, `for`).

**Examples:**

```p7
// Simple identifier binding
let x = 42;

// Tuple destructuring
let pair = (1, "hello");
let (n, s) = pair;
// n: int, s: string; pair is consumed

// Tuple-struct destructuring
struct Pair(pub int, pub int);
let p = Pair(10, 20);
let Pair(a, b) = p;

// Record-struct destructuring
struct Point(pub x: int, pub y: int);
let pt = Point(1, 2);
let Point(x = px, y = py) = pt;

// Mutable bindings via pattern
let mut (x, y) = get_pair();  // both x and y are mutable let mut slots
```

### 5.2 Shadowing

A `let` or `let mut` may introduce a new binding with the same name as an outer binding.

Rule: if `x` shadows `x`, the new binding MUST have the **same type** as the shadowed binding. The mutability may differ (i.e., a `let` binding may shadow a `let mut` binding and vice versa).

Example:
```p7
let a = 1;
{
  let a = 2;  // ok: same type int
  a
}
a
```

Example with `let mut`:
```p7
let x = 10;
{
  let mut x = 20;  // ok: same type int, but now mutable
  x = 30;          // ok: x is let mut
}
x  // still 10; outer x is immutable
```

**Import shadowing restriction (v1)**

A `let` or `let mut` binding must not shadow an import binding in the same scope.

Example (ERROR):
```p7
import std.collections.list;
let list = 5;  // ERROR: `list` shadows the import binding
```

Rationale: Import bindings are module-level and typically accessed throughout the module. Allowing local bindings to shadow them would make the imported module inaccessible within that scope, which is likely a programmer error. If the name is truly needed for a local variable, rename either the import (using `as`) or the variable.

Permitted workaround:
```p7
import std.collections.list as mylist;
let list = 5;  // ok: no shadowing
```

### 5.3 Mutation and identity

Protosept supports two forms of mutation, which operate on different axes and must not be confused:

1. **Binding-slot reassignment** (v1): `let mut` slots can be reassigned (§5.1). This is purely local mutation; `let mut` slots cannot be borrowed via `ref(...)`. `let mut` expresses that the *binding slot* is mutable — it does not affect the mutability of any object or value reached through the bound handle.

2. **Interior mutation through a handle**: In-place mutation through `box<T>` (and the mutable place forms based on it).
   - Assigning to a field is allowed through a box: `p.x = 1` is valid when `p: box<Point>`.
   - Assigning to a boxed-array element: `xs[i] = v` or `xs[i].field = v` when `xs: box<array<T>>`.
   - Value structs and value arrays are immutable.
   - Interior mutability is a property of the handle type (`box<T>`), not the binding mutability.

The distinction ensures that shared/observable mutation is always expressed via `box<T>`, while `let mut` provides convenient local reassignment for loop counters, accumulators, and similar use cases. A reader can always identify mutation capability by inspecting the type (`box<T>`) rather than the binding form.

#### Example: `let mut` in a loop accumulator

```p7
fn sum(arr: array<int>) -> int {
  let mut total = 0;
  for x in arr {
    total = total + x;  // ok: total is let mut
  }
  total
}
```

#### Example: `let mut` slots cannot be borrowed

```p7
let mut count = 0;
let r = ref(count);  // ERROR
```

This is ERROR because `let mut` slots are not addressable locations.

#### Example: `let mut` binding does not affect interior mutability

```p7
let c = box(Counter(0));
c.count = c.count + 1;   // ok: interior mutation via box<Counter>

let mut p = box(Point(1, 2));
p = box(Point(3, 4));    // ok: rebind the slot (let mut allows this)
p.x = 10;               // ok: interior mutation via box<Point> (independent of let mut)
```

---

## 6. Moves, copies, and `copy(x)`

### 6.1 Value-flow rule (move-by-default)

Whenever a value flows into a new slot or output position (binding, parameter, return, break-value, etc.):

- If the type satisfies `T: Copy`, the value is copied.
- Otherwise, the value is moved and the source becomes invalid to use (ERROR if used).

This rule applies uniformly to:
- `let` bindings
- argument passing
- returns
- `break expr` values
- iteration bindings (`for`)

### 6.2 The `structural_copy(x)` intrinsic

`structural_copy(x)` is a compiler intrinsic that performs a bitwise duplication of a value.

**Structural-copyable types** (types for which `structural_copy(x)` is well-typed):
- Primitives: `int`, `float`, `bool`, `char`, `unit`
- `string` (string data is reference-counted; copying duplicates the handle)
- `ref<T>` (view/handle copy; does not duplicate the referent)
- `box<T>` (handle copy; does not duplicate the heap allocation)
- `robox<T>` (handle copy; does not duplicate the heap allocation)
- `?T` iff `T` is structural-copyable
- `array<T>` iff `T` is structural-copyable
- `(T1, T2, ...)` (tuples) iff all components are structural-copyable
- `struct` iff all fields are structural-copyable
- `enum` iff all payload field types are structural-copyable

Using `structural_copy(x)` when `T` is not structural-copyable is ERROR.

#### 6.2.1 The `structural_eq` compiler intrinsic

`structural_eq<T>(a: ref<T>, b: ref<T>) -> bool` is a compiler intrinsic that performs structural equality comparison of two values through references.

**Signature:**
- Takes two references of the same type `T`
- Returns `bool`: `true` if the values are structurally equal, `false` otherwise

**Structural-eqable types** (types for which `structural_eq` is well-typed):

- **Primitives:**
  - `int`, `bool`, `char`, `unit`: bitwise equality
  - `float`: IEEE-754 equality semantics
    - `NaN == NaN` is `false` (NaN is not equal to itself)
    - `-0.0 == 0.0` is `true` (signed zeroes compare equal; note: they may produce different results in other operations, e.g., `1.0 / 0.0 != 1.0 / -0.0`)
    - Other values: bitwise equality of their IEEE-754 representation
  - `ptr`: identity equality (same address)

- **`string`**: content equality (compares UTF-8 byte sequences)

- **`?T`**: nullable equality
  - `null == null` is `true`
  - `null == Some(v)` is `false`
  - `Some(v1) == Some(v2)` recurses: `structural_eq(ref(v1), ref(v2))`
  - Requires `T` to be structural-eqable

- **Tuples `(T1, T2, ...)`**: component-wise equality
  - All components must be structural-eqable
  - `(a1, a2, ...) == (b1, b2, ...)` iff `structural_eq(ref(a1), ref(b1)) && structural_eq(ref(a2), ref(b2)) && ...`

- **`array<T>`**: element-wise equality
  - `T` must be structural-eqable
  - Arrays must have the same length
  - All corresponding elements must be equal: `structural_eq(ref(a[i]), ref(b[i]))` for all `i`

- **`struct` types**: field-wise equality
  - All fields must be structural-eqable
  - Compares all fields recursively: `structural_eq(ref(a.field), ref(b.field))` for each field

- **`enum` types**: variant and payload equality
  - All payload field types must be structural-eqable
  - First compares discriminants (which variant)
  - If variants match, compares payloads recursively via `structural_eq`

- **`ref<T>`**: referent value equality (NOT identity)
  - When comparing two `ref<T>` values, `structural_eq` compares the values at the referenced locations structurally
  - Does NOT compare addresses; compares the values at those addresses
  - Requires `T` to be structural-eqable (but does NOT require `T: Copy`)
  - This enables observational equality through references without dereferencing to a value

- **`box<T>` and `robox<T>`**: identity equality ONLY
  - Compares heap cell identity (same allocation), NOT contents
  - `box1 == box2` is `true` iff they point to the same heap cell
  - Deep content comparison is NOT performed
  - This is consistent regardless of whether `T` is structural-eqable

Using `structural_eq` when `T` is not structural-eqable is ERROR.

**Rationale:**
- `box<T>` and `robox<T>` use identity equality to preserve clear semantics: equality tests identity, not deep contents
- `ref<T>` uses referent value equality to enable observational equality without requiring `T: Copy`
- This design allows equality testing on non-Copy types through references

### 6.3 The `Copy` proto (built-in static proto)

`Copy` is a built-in **static proto** with a default method:

```p7
proto Copy {
  pub fn copy(ref self) -> Self {
    return structural_copy(*self);
  }
}
```

Note: `*self` dereferences the `ref<Self>` receiver to obtain the value. This is well-formed because the method is only callable when `Self: Copy`, ensuring the dereference succeeds per §7.1.

**Types satisfying `Copy` (`T: Copy`):**

A type `T` satisfies `Copy` iff:
1. `T` is structural-copyable (§6.2), AND
2. `T` explicitly opts in via `struct[Copy, ...] ...` or `enum[Copy, ...] ...` (for user-defined types), OR
3. `T` is a built-in type that satisfies `Copy` by default.

**Built-in types satisfying `Copy` by default:**
- Primitives: `int`, `float`, `bool`, `char`, `unit`
- `string`
- `ref<T>` (for any `T`)
- `box<T>` (for any `T`)
- `robox<T>` (for any `T`)
- `?T` iff `T: Copy` (by composition)
- `(T1, T2, ...)` (tuples) iff all components satisfy `Copy`
- `array<T>` iff `T: Copy`

**User-defined types:**
- `struct` types satisfy `Copy` **only if** they opt in via `struct[Copy, ...] ...` **and** all fields are structural-copyable.
- `enum` types satisfy `Copy` **only if** they opt in via `enum[Copy, ...] ...` **and** all payload field types are structural-copyable.

Listing `Copy` in a struct/enum conformance when the structural-copyable requirement is not met is ERROR.

### 6.4 Explicit copying: `copy(x)`

`copy(x)` is the explicit copying operation.

**Semantics:**
- `copy(x)` is well-typed iff `T: Copy` where `T` is the type of `x`.
- It returns a value of type `T`.
- `copy(x)` desugars to `T.copy(ref(x_tmp))` where `x_tmp` is the addressable location for `x`:
  - If `x` is already an addressable location, `x_tmp` is `x`.
  - If `x` is not addressable (e.g., a temporary), a **materialized temporary slot** is created (§0), and `x_tmp` refers to that slot.

This enables `copy(some_expr)` to work uniformly whether `some_expr` is a variable, field access, or any other value-producing expression.

**Rationale:**
- `Copy` is a proto with a method; calling `copy(x)` invokes that method via the standard method-call mechanism with receiver temporary materialization.


#### 6.4.1 The `Eq` static proto

`Eq` is a built-in **static proto** that enables equality testing via the `==` and `!=` operators.

**Proto declaration:**

```p7
proto Eq {
  pub fn eq(ref self, other: ref<Self>) -> bool {
    return structural_eq(self, other);
  }
}
```

**Default implementation:**
- The default `eq` method uses `structural_eq` to compare the referents
- Takes `ref self` (borrowed receiver) and `other: ref<Self>` (borrowed argument)
- Returns `bool`
- Does NOT require `Self: Copy`; observes values through references

**Types satisfying `Eq` (`T: Eq`):**

A type `T` satisfies `Eq` iff:
1. `T` is structural-eqable (§6.2.1), AND
2. `T` explicitly opts in via `struct[Eq, ...] ...` or `enum[Eq, ...] ...` (for user-defined types), OR
3. `T` is a built-in type that satisfies `Eq` by default.

**Built-in types satisfying `Eq` by default:**
- Primitives: `int`, `float`, `bool`, `char`, `unit`, `ptr`
- `string`
- `ref<T>` (for any `T` that is structural-eqable)
- `box<T>` (for any `T`; uses identity equality)
- `robox<T>` (for any `T`; uses identity equality)
- `?T` iff `T: Eq` (by composition)
- `(T1, T2, ...)` (tuples) iff all components satisfy `Eq`
- `array<T>` iff `T: Eq`

**User-defined types:**
- `struct` types satisfy `Eq` **only if** they opt in via `struct[Eq, ...] ...` **and** all fields are structural-eqable.
- `enum` types satisfy `Eq` **only if** they opt in via `enum[Eq, ...] ...` **and** all payload field types are structural-eqable.

Listing `Eq` in a struct/enum conformance when the structural-eqable requirement is not met is ERROR.

**Rationale:**
- `Eq` is a static proto (not object-safe; `Self` appears in parameter type)
- Default implementation leverages `structural_eq` for observation through references
- Enables equality on non-Copy types without requiring value-level dereference
- `box<T>` and `robox<T>` use identity equality regardless of `T` to maintain clear semantics


#### 6.4.2 The `Display` proto (built-in formatting proto)

`Display` is a built-in proto used for user-facing string formatting (notably, interpolated string literals; see §4.3.1).

**Proto declaration:**

```p7
proto Display {
  pub fn display(ref self) -> string;
}
```

`Display` is an **object proto** (§18.2). However, interpolated string literals do not require or imply dynamic dispatch; they are specified in terms of `Display.display` as a compile-time requirement on each interpolation hole.

**Types satisfying `Display` (`T: Display`):**

A type `T` satisfies `Display` iff:
1. `T` explicitly opts in via `struct[Display, ...] ...` or `enum[Display, ...] ...` (for user-defined types), AND
2. After proto default-method injection (§18.3), `T` provides a method matching `display(ref self) -> string`, OR
3. `T` is a built-in type that satisfies `Display` by default (below).

**Built-in types satisfying `Display` by default (v1):**
- Primitives: `int`, `float`, `bool`, `char`, `unit`, `ptr`
- `string`

**Rationale:**
- Allows convenient formatting in interpolated strings (e.g., `f"{x}"` where `x: int`) without introducing implicit conversion or assignability between unrelated types (e.g., `int` is still not assignable to `string`).
- Keeps formatting behavior explicit and discoverable via a named proto and method (`Display.display`).

### 6.5 The `Send` static proto

`Send` is a built-in **static proto** indicating a deep-copyable pure value with no shared identity/aliasing.

Types satisfying `Send` (`T: Send`) in v1:
- Primitives
- `string`
- `array<T>` iff `T: Send`
- User-defined `enum` iff all payload field types satisfy `Send`
- User-defined `struct` iff all fields satisfy `Send`

Types that do NOT satisfy `Send`:
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
  - if `T: Copy`, `*r` yields a copy;
  - otherwise, `*r` causes an ERROR.

**Operations on `ref<T>` values:**

- Member access (`r.field`) and method calls (`r.method(...)`) are permitted without copying `T`:
  - These operations access the referent location directly.
  - Only explicit dereference (`*r`) is restricted by the `T: Copy` requirement.

### 7.2 Taking views

`ref(place)` produces a `ref<T>` when `place` is an addressable location of type `T`.

Requirements:
- `place` MUST be an addressable location (see §0). Note that `let mut` slots are not addressable.

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
- captured by closures (§9.7)
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
  - **Desugaring**: `box(expr)` desugars to `box<T>.new(expr)` where `T` is the type of `expr`. `box<T>.new` is an intrinsic method declared in the prelude.

- Read / deref: `*b`
  - `*b` is an **addressable location** (place expression) referring to the boxed contents.
  - If `T: Copy`, `*b` as a value expression yields a copy of type `T`.
  - If `T` does not satisfy `Copy`, using `*b` as a value expression (moving out) is ERROR in v1.
  - Rationale: boxes are aliasable; moving out implicitly would require moved-out states or uniqueness.

- Write: `*b = expr`
  - Requires `expr: T`.
  - Overwrites the boxed contents.

- Replace: `replace(b, new_value) -> T`
  - Writes `new_value` into the box and returns the previous value.
  - This is the way to extract non-Copy values from a box without leaving it uninitialized.

- **Borrowing boxed contents**: `ref(*b)`
  - Produces a `ref<T>` view of the boxed contents.
  - Permitted for **any** `T`, including types that do not satisfy `Copy`.
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
  - If `T: Copy`, `*rb` as a value expression yields a copy of type `T`.
  - If `T` does not satisfy `Copy`, using `*rb` as a value expression is ERROR (mirroring the `box<T>` rule).

- Write: `*rb = expr` is **ERROR** when `rb: robox<T>`.

- **Borrowing boxed contents**: `ref(*rb)`
  - Produces a `ref<T>` view of the boxed contents.
  - Permitted for **any** `T`, including types that do not satisfy `Copy`.
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

### 7.6 Ephemeral mutable-place semantics (v1)

A **mutable place** (see §0) is a syntactic expression that may appear on the left side of an assignment statement, or as the receiver of a `ref mut self` method call. Mutable places are **ephemeral**: they are not storable values, cannot be passed as arguments, cannot be returned from functions, and have no first-class mutable-reference type in v1.

#### 7.6.1 Mutable place forms

The following expressions are mutable places in v1:

| Expression | Requirement | Note |
|---|---|---|
| `x` | `x` introduced by `let mut` | binding-slot reassignment |
| `*b` | `b: box<T>` | boxed deref (write) |
| `b.field` | `b` is a mutable place of struct type `S`, `field` is a field of `S` | nested field of mutable place |
| `b[i]` | `b: box<array<T>>` | element of boxed array |
| `b[i].field` | `b: box<array<T>>`, element type is struct `S`, `field` is a field of `S` | nested field of boxed-array element |

The mutable place forms compose: if `b[i]` is a mutable place of struct type `S`, then `b[i].field` is also a mutable place for any visible field `field` of `S`.

The following are **not** mutable places:
- `let`-introduced slots (immutable bindings)
- Parameter slots (parameters are not reassignable in v1)
- `ref<T>` values (read-only borrowed views)
- `*rb` / `rb.field` where `rb: robox<T>` (read-only boxes)
- `a[i]` where `a: array<T>` (value arrays are immutable)

#### 7.6.2 Restriction: ephemeral and non-storable

Mutable places are ephemeral and exist only in specific syntactic positions:

- A mutable place may appear as the `place` in an assignment statement `place = expr;` (§10.2).
- A mutable place may appear as the receiver of a `ref mut self` method call `place.method(args...)` (§11.3.2).
- A mutable place may NOT be stored: there is no `ref_mut<T>` value form in v1.
- A mutable place may NOT be passed as a function argument (beyond the implicit `ref mut self` receiver desugaring).
- A mutable place may NOT be returned from a function.

#### 7.6.3 Local exclusivity restriction for boxed-array element places

When `xs[i]` is used as a mutable place within an expression (either on the left of `=` or as the receiver of a `ref mut self` call), the same expression MUST NOT also perform structural mutation of the containing array `xs`. Structural mutations include: calls to `push`, `pop`, `insert`, `remove`, or any operation that may reallocate or resize the array, and assignment to `xs` itself.

**Examples:**

```p7
struct Point(pub x: int, pub y: int) {
  pub fn shift(ref mut self, dx: int) {
    self.x = self.x + dx;
  }
}

let xs = box([Point(1, 2), Point(3, 4)]);

// OK: assign to element
xs[0] = Point(10, 20);

// OK: assign to field of element
xs[0].x = 10;

// OK: call ref mut self method on element
xs[0].shift(5);

// ERROR: structural mutation (push) of xs while xs[i] is in use as mutable place
// xs[0] = xs.pop();           // ERROR: pop() structurally mutates xs in the same expression
// xs.push(Point(0, 0));       // ok as a standalone statement; ERROR only when combined with xs[i] mutable-place use in same expression
```

Rationale: This restriction prevents an element-place from being invalidated by a concurrent structural change to the containing array within a single expression. No lifetime tracking is required because the restriction is purely local (expression-level).

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
- closure literals (§9.7)

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
- `if ... else ...`, `try`, `match`, `loop`, and closure literals (`(...) => ...`) are expression forms written with blocks and bind looser than any operator above.
- `if` without `else` is statement-only and does not participate in operator precedence.
- Prefix `!x` (logical NOT) and postfix `x!` (force unwrap) are distinct by position.

#### 9.3.1 Equality operators: `==` and `!=`

The equality operators `==` and `!=` test structural equality and inequality.

**Typing:**
- `a == b` is well-typed iff:
  - The types of `a` and `b` unify to some type `T`
  - `T: Eq` (the type satisfies the `Eq` proto; see §6.4.1)
- The result type is `bool`
- `a != b` has the same typing rules as `a == b`

If `T` does not satisfy `Eq`, using `==` or `!=` is ERROR.

**Desugaring:**

`a == b` desugars to `T.eq(ref(a_tmp), ref(b_tmp))` where:
- `T` is the unified type of `a` and `b`
- `a_tmp` and `b_tmp` are addressable locations for `a` and `b`:
  - If the operand is already an addressable location, that location is used
  - If the operand is not addressable (e.g., a temporary expression result), a **materialized temporary slot** (§0) is created, and the temporary refers to that slot
- Evaluation occurs exactly once for each operand (no re-evaluation)

`a != b` desugars to `!(a == b)` (logical negation of equality).

**Examples:**
```p7
// Primitive equality
let x = 42;
let y = 42;
x == y  // desugars to: int.eq(ref(x), ref(y)) => true

// String equality (content comparison)
let s1 = "hello";
let s2 = "hello";
s1 == s2  // true (content equality)

// Box identity equality
let b1 = box(42);
let b2 = box(42);
b1 == b2  // false (different heap cells, identity equality)
let b3 = b1;
b1 == b3  // true (same heap cell)

// Reference value equality
let x = 10;
let r1 = ref(x);
let r2 = ref(x);
r1 == r2  // true (compare referent values, not addresses)
```

**Cross-references:**
- `Eq` proto definition: §6.4.1
- `structural_eq` intrinsic: §6.2.1
- Match pattern equality: §9.6.1

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

#### 9.6.1 Patterns

A **pattern** describes how a value is matched and optionally how bindings are introduced from its substructure. Patterns are used in:

- `match` arms (§9.6)
- `let` and `let mut` bindings (§5.1)
- `for` iteration bindings (§10.3)
- `try ... else` handler arms (§14.2)
- [[TODO]] function/closure parameters (deferred to a future version)

##### 9.6.1.1 Pattern forms

Supported pattern forms:

- **Wildcard**: `_` — matches any value; introduces no binding.
- **Binding**: `name` — matches any value and binds `name` to the matched value. The legacy `name: pattern` form (where `name` binds the matched sub-value) is an alternative syntax that remains fully supported; `name` is not deprecated.
- **Literal**: `42`, `3.14`, `"hi"`, `true`, `false`, `null` — matches the specific literal value; requires `Eq` (see below).
- **Path**: `EnumName.VariantName` — matches a unit enum variant; requires `Eq`.
- **Tuple**: `(p1, p2, ..., pn)` where `n ≥ 2` — matches a tuple value by position.
- **Tuple-struct**: `TypeName(p1, ..., pn)` — matches a tuple-struct value by positional field index.
- **Record-struct**: `TypeName(f1 = p1, f2 = p2, ..., fn = pn)` — matches a record-struct value by named fields. If a field pattern is written as just `fname` (without `= sub_pattern`), it is shorthand for `fname = fname` (binding the field value to a variable with the same name as the field).
- **Enum payload variant**: `EnumName.Variant(p1, ..., pn)` — matches a specific payload variant and destructures its payload by position.

Grammar sketch:

```
pattern            := '_'
                    | ident
                    | literal
                    | path
                    | '(' pattern ',' pattern (',' pattern)* ','? ')'
                    | path '(' pattern_list? ')'
                    | path '(' field_pattern_list ')'

pattern_list       := pattern (',' pattern)* ','?
field_pattern      := ident ('=' pattern)?
field_pattern_list := field_pattern (',' field_pattern)* ','?

named_pattern      := [ident ':'] pattern   -- alternative form; equivalent to ident binding sub-pattern
```

The `path` in a nominal pattern must resolve to a struct name, an enum type, or a qualified enum variant name.

**Equality requirement for literal and path patterns:**

Literal patterns and path patterns for unit enum variants test equality and require the scrutinee type to satisfy `Eq`. Structural patterns (tuple, tuple-struct, record-struct, enum payload) deconstruct by type structure and do **not** require `Eq`.

##### 9.6.1.2 Irrefutable and refutable patterns

A pattern is **irrefutable** for a type `T` if it is statically guaranteed to match every value of type `T`. Otherwise it is **refutable**.

**Irrefutable patterns:**
- `_` (wildcard)
- `name` (binding / named wildcard)
- Tuple patterns `(p1, ..., pn)` where all sub-patterns are irrefutable and arity matches the tuple type.
- Tuple-struct patterns `TypeName(p1, ..., pn)` where all sub-patterns are irrefutable and arity matches the struct definition.
- Record-struct patterns `TypeName(f1 = p1, ..., fn = pn)` that mention every field of the struct at least once and all sub-patterns are irrefutable. (In v1, mentioning every field *exactly* once is required; partial record patterns with `..` are deferred to a future version.)

**Refutable patterns:**
- Literal patterns.
- Path patterns for enum unit variants.
- Enum payload variant patterns (a multi-variant enum may not be that variant).
- Any pattern containing a refutable sub-pattern.

**Contextual rules:**
- `let`, `let mut`, and `for` MUST use irrefutable patterns. Using a refutable pattern in these contexts (e.g., `let Result.Ok(x) = expr;`) is ERROR.
- `match` and `try ... else` handler arms may use refutable patterns.

##### 9.6.1.3 Pattern typing

A pattern is checked against the **scrutinee type** `T`:

- `_` and `name` are valid for any type `T`.
- Literal patterns require that the literal's type is compatible with `T` and that `T: Eq`.
- Path patterns for unit enum variants require `T` to be that enum type and `T: Eq`.
- Tuple patterns `(p1, ..., pn)` require `T` to be a tuple type `(T1, ..., Tn)` of matching arity; each `pi` is checked against `Ti`.
- Tuple-struct patterns `TypeName(p1, ..., pn)` require `T = TypeName` where `TypeName` is a tuple struct; each `pi` is checked against the type of positional field `i`.
- Record-struct patterns `TypeName(f1 = p1, ..., fn = pn)` require `T = TypeName` where `TypeName` is a record struct; each field `fi` must exist in `TypeName` and `pi` is checked against `fi`'s declared type. In v1, every field of the struct MUST be mentioned exactly once (partial record patterns with `..` are not supported).
- Enum payload variant patterns `EnumName.Variant(p1, ..., pn)` require `T = EnumName`; the variant must exist and have exactly `n` payload fields; each `pi` is checked against the type of payload field `i`.

**Field visibility:**

For struct patterns (tuple-struct and record-struct), every mentioned field MUST be visible at the pattern site, applying the same rules as field access (§12.4). Destructuring a non-visible field is ERROR.

**Example (visibility restriction):**
```p7
struct Secret(pub x: int, y: int);  // y is private

// outside the declaring module:
let Secret(x = a, y = b) = s;  // ERROR: field y is not visible
let Secret(x = a) = s;         // ERROR in v1: all fields must be mentioned
```

##### 9.6.1.4 Binding and value-flow semantics

When a pattern introduces bindings from sub-values, each bound sub-value flows into its binding according to the standard value-flow rule (§6.1):
- If the sub-value's type satisfies `Copy`, it is copied.
- Otherwise it is moved.

The matched source value is consumed exactly once. Borrow patterns are not supported in v1; deconstruction does not introduce implicit borrowing. To destructure a value held through `ref<T>`, `box<T>`, or `robox<T>`, the inner type `T` must satisfy `Copy`, because applying a pattern requires materializing the inner value, which for these handle types requires a dereference — and dereferencing a non-`Copy` inner type is not permitted.

**Pattern examples:**

```p7
// Wildcard — no binding
match x {
  _ => "anything",
}

// Binding — whole value bound to n
match x {
  0 => "zero",
  n => f"non-zero: {n}",
}

// Legacy named wildcard (still supported)
match x {
  0    => "zero",
  n: _ => f"non-zero: {n}",
}

// Enum payload variant patterns
enum Result<T, E>(
  Ok: T,
  Err: E,
);

fn describe(r: Result<int, string>) -> string {
  match r {
    Result.Ok(n)  => f"ok: {n}",
    Result.Err(e) => f"err: {e}",
  }
}

// Tuple pattern in match
match pair {
  (0, s) => f"zero with {s}",
  (n, s) => f"{n} with {s}",
}

// Record-struct pattern in match
struct Point(pub x: int, pub y: int);
match pt {
  Point(x = 0, y = 0) => "origin",
  Point(x = px, y = py) => f"({px}, {py})",
}

// Enum with unit and payload variants
enum Status(
  Pending,
  Active: int,
  Failed: (string, int),
);

match s {
  Status.Pending        => "pending",
  Status.Active(code)   => f"active: {code}",
  Status.Failed(msg, n) => f"failed: {msg} ({n})",
}
```

#### 9.6.2 Evaluation and control flow

- The `scrutinee` expression is evaluated exactly once.
- Arms are tried in source order.
- For each arm:
  - If `pattern` matches, all bindings introduced by the pattern are brought into scope, then the arm expression is evaluated and becomes the result of the `match`.
  - If `pattern` does not match, the next arm is tried.

#### 9.6.3 Typing

- All arm expressions MUST have the same type in v1.
- The `match` expression has that common type.

#### 9.6.4 Exhaustiveness (v1)

`match` MUST be exhaustive.

- If it is not statically provable that some arm matches, the program is ill-formed (ERROR).
- The simplest portable way to be exhaustive is to include a final wildcard arm `_ => ...`.
- For enum types, listing all variants (including payload variants) exhaustively is also accepted without a wildcard.

Example:
```p7
fn classify(x: int) -> int {
  match x {
    0 => 0,
    n => n,
  }
}
```

### 9.7 Closure literals

A closure literal creates an anonymous callable value. Closures are first-class values: they can be bound to variables, passed as arguments, returned from functions, and stored in data structures.

#### 9.7.1 Syntax

```
(p1: T1, p2: T2, ...) => expr
(p1: T1, p2: T2, ...) => { block }
[effect1, effect2, ...] (p1: T1, p2: T2, ...) => expr
[effect1, effect2, ...] (p1: T1, p2: T2, ...) => { block }
```

- Parameters use the same `name: Type` syntax as function declarations; all parameter types MUST be explicit in v1.
- The body is either a single expression or a block (following the same rules as block expressions, §9.1).
- The return type is inferred from the body expression. To constrain the return type, annotate the binding with a function type (§3.9): `let f: fn(int) -> int = (x: int) => x + 1;`.
- The optional effect list uses the same `[effect1, effect2, ...]` syntax as function declarations (§11.2), placed before the parameter list. Effects are a **set**: order is not significant, duplicates are ERROR.

**Effect rules:**

- A closure with **no effect list** has an **empty effect set** — it is pure and MUST NOT throw, suspend, or perform any declared effect.
- If a closure body may throw (via `throw` or by calling a throwing function with `try` propagation), the closure MUST declare `throws` or `throws<E>` in its effect list.
- If a closure body may suspend (via `yield`), the closure MUST declare `suspend` in its effect list.

This maintains the same explicitness guarantee as named functions: the effect set of a callable is always visible from its declaration.

**Disambiguating `=>` from `match` arms:**

The `=>` token also appears in `match` and `try ... else { ... }` arm syntax (§9.6, §14.2). Ambiguity does not arise because:
- In `match` / `try ... else` arms, the pattern appears to the left of `=>` without a preceding `(params)` or `[effects]` form.
- Closure literals require a parenthesized parameter list (possibly empty) immediately before `=>`, optionally preceded by an effect list in brackets.

#### 9.7.2 Typing

A closure literal `(p1: T1, ...) => body` with effect set `E` has type `fn[E](T1, ...) -> R`, where `R` is the type of the body expression or the tail expression of the body block.

A closure with no effect list has type `fn(T1, ...) -> R`.

Closures support bidirectional typing (§0.1):
- **Synthesize (↑)**: When all parameter types are explicit, the closure's type is synthesized from its parameter types, inferred return type, and declared effects.
- **Check (↓)**: A closure may be checked against an expected function type. In v1, all parameter types must still be explicit; the expected type is used only to verify compatibility (not to infer parameter types).

[[TODO]] Consider allowing parameter type inference from expected type in a future version.

#### 9.7.3 Capture semantics

Closures may reference bindings from enclosing scopes. These references are called **captures**.

**By-value capture (v1):**

All captures in v1 use **by-value** semantics. When a closure captures a binding, the value flows into the closure at the point of closure creation, following the standard value-flow rule (§6.1):
- If the captured value's type satisfies `Copy`, the value is copied into the closure.
- Otherwise, the value is **moved** into the closure and the original binding becomes invalid (ERROR if subsequently used).

**Capture restrictions:**

- `ref<T>` values MUST NOT be captured (§7.3). A closure that references a `ref<T>` binding from an enclosing scope is ERROR.
- `box<T>` values may be captured. Because `box<T>` is structural-copyable and satisfies `Copy` (§6.3), capturing a `box<T>` copies the handle into the closure. Both the closure and the enclosing scope share the same heap cell — mutations through either handle are visible through the other.
- `robox<T>` values may be captured (same handle-copy semantics as `box<T>`).

**Implicit capture set:**

The compiler determines the capture set from the free variables in the closure body. There is no explicit capture list syntax in v1.

[[TODO]] Consider explicit capture list syntax (e.g., `[captures] (params) => body`) for auditability in a future version.

#### 9.7.4 Calling closures

A closure value is called with standard call syntax:

```p7
let f = (x: int) => x + 1;
let result = f(5);   // 6
```

Effectful closures obey the same call-site explicitness rules as named functions:

- **Throwing closures** MUST be called under `try` (§14.3). A bare call to a closure whose type includes `throws` or `throws<E>` is ERROR.
- **Suspending closures** follow the same restrictions as suspending functions (§21.2): they may only be called from within a function (or closure) that also has the `suspend` effect.

```p7
let parse: fn[throws<ParseError>](string) -> int = ...;
let n = try parse("42");   // ok: wrapped in try
// let m = parse("42");    // ERROR: throwing callable must be called under try
```

#### 9.7.5 Examples

**Pure closure:**
```p7
let double = (x: int) => x * 2;
let result = double(5);   // 10

let add = (a: int, b: int) => a + b;
let sum = add(3, 4);      // 7
```

**Throwing closure with `try` at call site:**
```p7
enum ParseError(
  InvalidFormat,
);

fn[throws<ParseError>] parse_int(s: string) -> int { ... }

let safe_parse = [throws<ParseError>] (s: string) => {
  try parse_int(s)
};

fn handle_input(s: string) -> int {
  try safe_parse(s) else {
    _: ParseError.InvalidFormat => 0,
  }
}
```

**Suspending closure:**
```p7
let task = [suspend] () => {
  yield;
  // ... continues after resume
  yield;
};

// task has type fn[suspend]() -> unit
// can be spawned:
spawn task();
```

**Closure capturing a `box<T>`:**
```p7
struct Counter(value: int);

let c = box(Counter(0));
let increment = () => {
  c.value = c.value + 1;
};

increment();           // mutates the boxed Counter
increment();
let n = c.value;       // 2 — both `c` and the closure share the same box
```

**Closure as a function parameter:**
```p7
fn apply(f: fn(int) -> int, x: int) -> int {
  return f(x);
}

let result = apply((x: int) => x * x, 10);   // 100
```

**Throwing closure passed to a higher-order function:**
```p7
fn try_apply(f: fn[throws](int) -> int, x: int) -> ?int {
  try f(x) else null
}
```

---

## 10. Statements

Statement forms:
- `let` binding: `let x = expr;`
- `let mut` binding: `let mut x = expr;`
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
- declarations where allowed.

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
- `place` must be a mutable place (§7.6.1):
  1. A `let mut` slot: `x = expr` where `x` was introduced by `let mut`.
     - `expr` MUST have the same type as the slot.
  2. A boxed deref: `*b = expr` where `b: box<T>`.
  3. A boxed field: `b.field = expr` where `b: box<S>` or `b` is a mutable place of struct type `S`.
  4. A boxed-array element: `b[i] = expr` where `b: box<array<T>>`.
     - `expr` MUST have type `T`.
  5. A nested field of a boxed-array element: `b[i].field = expr` where `b: box<array<T>>` and the element type is struct `S` with a visible field `field`.
     - `expr` MUST have the declared type of `field`.
- Assignment to read-only boxes is ERROR:
  - `*rb = expr` where `rb: robox<T>` is ERROR.
  - `rb.field = expr` where `rb: robox<S>` is ERROR.
- Assigning to a `let` slot (`x = expr` where `x` was introduced by `let`) is ERROR.
- Assignment does not produce a value.

#### 10.2.1 Local exclusivity for boxed-array element assignment

When the `place` in an assignment is a boxed-array element place (`b[i]` or `b[i].field`), the assignment expression MUST NOT also structurally mutate `b` (e.g., by calling `push`, `pop`, `insert`, or `remove` on `b`, or by assigning to `b` itself). This is a local, expression-scope restriction; it does not require lifetime tracking.

```p7
let xs = box([1, 2, 3]);

xs[0] = 10;              // OK
xs[0] = xs[1];           // OK: right-hand side reads xs[1], left side writes xs[0]
// xs[xs.push(4)] = 5;  // ERROR: structural mutation of xs in the same expression
```

### 10.3 `for` statement (v1)

Form:
```p7
for pattern in expr { body }
```

`expr` MUST have type:
- `array<T>` (then the iteration element type is `T`), or
- `string` (then the iteration element type is `char`).

`pattern` MUST be an irrefutable pattern (§9.6.1.2) for the iteration element type; otherwise ERROR.

Semantics:
- `expr` is evaluated once.
- `body` executes once per element/character, in order.
- `break` and `continue` behave as in `loop`.

Binding rule:
- All bindings introduced by `pattern` are fresh each iteration, following the standard value-flow rule (§6.1).
- When `pattern` is a plain identifier `x`, this is the common form `for x in expr { ... }`.

**Examples:**

```p7
// Simple identifier binding (existing form)
for x in arr {
  total = total + x;
}

// Tuple destructuring
for (k, v) in entries {
  process(k, v);
}

// Record-struct destructuring
struct Point(pub x: int, pub y: int);
for Point(x = px, y = py) in points {
  draw(px, py);
}
```

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
  - If `recv` has type `Self` but is NOT an addressable location (i.e., a temporary): the compiler MUST materialize the receiver into an implicit immutable temporary `let` slot and then borrow that slot for the call. Desugars to:
    ```
    let __tmp_recv = recv;
    Type.method(ref(__tmp_recv), args...)
    ```
    where `__tmp_recv` is a compiler-generated name. The materialized temporary lives until the method-call expression completes.
  - If `recv` has type `box<Self>`: desugars to `Type.method(ref(*recv), args...)`. The receiver `recv` may be any value (including temporaries), as the borrow is taken of the dereferenced contents `*recv`.
  - If `recv` has type `robox<Self>`: desugars to `Type.method(ref(*recv), args...)`. The receiver `recv` may be any value (including temporaries), as the borrow is taken of the dereferenced contents `*recv`.
  - If `recv` already has type `ref<Self>`: it is passed directly to the `ref self` parameter without desugaring.
  - The receiver is evaluated exactly once.

**Restrictions:**

- Applies only to methods with `ref self` receivers; does NOT apply to free functions with `ref<T>` parameters.

**Normative note: Evaluation order and temporary lifetime:**

- The receiver expression is evaluated exactly once, before the method arguments.
- When receiver temporary materialization occurs (for `Self`-typed temporaries), the materialized temporary slot lives until the method-call expression completes.
- The compiler MAY optimize away the materialized temporary as long as observable semantics are preserved (e.g., if the method does not actually retain the reference beyond its execution).

**Example:**

Consider a struct `Point` with a `ref self` method `distance`:

```p7
struct Point(x: int, y: int) {
  pub fn distance(ref self) -> int {
    return self.x * self.x + self.y * self.y;
  }
}

fn other() -> Point {
  return Point(3, 4);
}

let d = other().distance();  // ok: temporary receiver is materialized
```

The call `other().distance()` desugars to:
```p7
let __tmp_recv = other();
Point.distance(ref(__tmp_recv))
```

The temporary `__tmp_recv` is valid for the duration of the method call, and the result is assigned to `d`.

**Rationale:**

This sugar reduces ceremony at method call sites while maintaining explicit borrowing for free function calls, providing ergonomics where method chaining and fluent APIs are common.

#### 11.3.2 Method-call auto-borrow sugar for `ref mut self` receivers

For method calls on mutable places, p7 provides auto-borrow sugar when the method has a `ref mut self` receiver (§11.4):

**Sugar rules:**

- `recv.method(args...)` where `method` has a `ref mut self` receiver desugars as follows:
  - If `recv` is a `let mut` slot of type `Self`: desugars to `Type.method(ref_mut(recv), args...)` — the compiler takes an ephemeral mutable borrow of the slot for the duration of the call.
  - If `recv` has type `box<Self>`: desugars to `Type.method(ref_mut(*recv), args...)` — the mutable borrow is taken of the dereferenced contents. The receiver may be any `box<Self>` value (including non-`let-mut` bindings); the box itself provides the mutable place via `*recv`.
  - If `recv` is a boxed field `b.field` where `b: box<S>` and the field type is `Self`: desugars to `Type.method(ref_mut(b.field), args...)`.
  - If `recv` is `b[i]` where `b: box<array<Self>>`: desugars to `Type.method(ref_mut(b[i]), args...)`.
  - If `recv` is `b[i].field` where `b: box<array<S>>` and the field type is `Self`: desugars to `Type.method(ref_mut(b[i].field), args...)`.
  - Calling a `ref mut self` method on a `robox<Self>` value is ERROR (`*rb` is not a mutable place when `rb: robox<T>`).

**Restrictions:**

- Applies only to methods with `ref mut self` receivers; does NOT apply to methods with `ref self`, `box self`, or value `self` receivers.
- The receiver expression must either be a mutable place (§7.6.1) or an expression of type `box<Self>` (from which `*recv` is a mutable place). Calling a `ref mut self` method on any other expression is ERROR.
- The `ref_mut(...)` desugaring in the rules above is an **internal notation only** — there is no user-visible `ref_mut<T>` type or `ref_mut(...)` expression in v1. The mutable borrow is always implicit and ephemeral.
- The mutable borrow exists only for the duration of the method call expression; it does not outlive the call.
- The local exclusivity restriction of §7.6.3 applies: when the receiver is a boxed-array element place, the same expression must not also structurally mutate the containing array.

**Example:**

```p7
struct Point(pub x: int, pub y: int) {
  pub fn shift(ref mut self, dx: int, dy: int) {
    self.x = self.x + dx;
    self.y = self.y + dy;
  }
  pub fn get_x(ref self) -> int {
    return self.x;
  }
}

// Via let mut slot
let mut p = Point(1, 2);
p.shift(3, 4);           // ok: desugars to Point.shift(ref_mut(p), 3, 4)

// Via boxed array element
let xs = box([Point(0, 0), Point(10, 10)]);
xs[0].shift(1, 1);       // ok: desugars to Point.shift(ref_mut(xs[0]), 1, 1)
xs[1].x = 5;             // ok: direct field assignment (§10.2)
let v = xs[0].get_x();   // ok: ref self method still works (§11.3.1)
```

**Normative note:**

The `ref_mut(...)` notation in the desugaring rules above is internal specification notation. The user never writes `ref_mut` in source. The `ref mut self` receiver form is solely a method receiver syntax (§11.4), not a general type constructor.

### 11.4 Method receivers (v1)

Methods on structs, enums, and protos may declare a receiver parameter. The receiver is the first parameter and uses special syntax.

**Receiver position rule:**

- The receiver **must** be the first parameter in a method declaration.
- `ref` and `box` are interpreted as receiver modifiers **only** when they immediately precede `self` in the receiver position (e.g., `ref self`, `box self`).
- In all other contexts (including when not immediately followed by `self`), `ref` and `box` are ordinary identifiers.

**Receiver forms:**

1. `self` – by-value receiver:
   - Type: `Self` (the declaring type).
   - Passes ownership; subject to value-flow rules (§6.1).

2. `self: ref<Self>` or shorthand `ref self` – borrowed receiver:
   - Type: `ref<Self>`.
   - Caller passes a read-only view of an addressable location.
   - Method-call syntax automatically applies the auto-borrow sugar (§11.3.1).

3. `ref mut self` – ephemeral mutable-borrowed receiver:
   - Denotes ephemeral mutable borrowed access to `Self` for the duration of the method call.
   - The caller must supply a mutable place of type `Self` (§7.6.1), or an expression of type `box<Self>` (from which the compiler takes a mutable borrow of `*recv`); the compiler applies the auto-borrow sugar (§11.3.2).
   - This form is **receiver-only** in v1: `ref mut` is not a general type constructor, and there is no first-class `ref_mut<T>` value type.
   - `ref mut self` is distinct from `let mut` (binding mutability) and from `box self` (escaping identity): it belongs to the borrow/access-capability axis and is strictly ephemeral.
   - Within the method body, `self` may be used to read and write fields of `Self`, subject to field-visibility rules (§12.1.1).

4. `self: box<Self>` or shorthand `box self` – boxed receiver:
   - Type: `box<Self>`.
   - Caller passes a boxed handle to the instance.
   - The boxed handle satisfies `Copy` (§6.3); passing does not move the box itself.
   - The box's contents remain aliased; multiple calls to methods on the same box see shared state.

**Rules:**

- The receiver is the first parameter; it is written before other parameters without a trailing comma.
- No implicit boxing occurs to satisfy a receiver:
  - A method with `self: box<Self>` requires the caller to have `box<Self>`, not just `Self`.
- For methods with `ref self` receivers, the auto-borrow sugar (§11.3.1) applies at method call sites for `box<Self>` and `robox<Self>` receivers.
- For methods with `ref mut self` receivers, the auto-borrow sugar (§11.3.2) applies at method call sites; the receiver expression must be a mutable place or a `box<Self>` value.
- Boxed receivers (`self: box<Self>`) pass the box handle, which satisfies `Copy`. This allows multiple method calls on the same box without moving the box itself.
- Calling a method with a `box self` receiver on a `robox<Self>` value is ERROR (capability mismatch).
- Calling a `ref mut self` method on a non-mutable-place, non-`box<Self>` expression is ERROR.

**Example:**
```p7
struct Counter(pub count: int) {
  pub fn increment(ref mut self) {
    self.count = self.count + 1;
  }
  pub fn get(ref self) -> int {
    return self.count;
  }
  pub fn reset(box self) {
    self.count = 0;
  }
}

// Via let mut slot
let mut c = Counter(0);
c.increment();       // ok: ref mut self auto-borrow (§11.3.2)
c.increment();       // ok: can call again on same mutable place
let n = c.get();     // ok: ref self auto-borrow (§11.3.1); n == 2

// Via boxed array
let cs = box([Counter(0), Counter(10)]);
cs[0].increment();   // ok: ref mut self on boxed-array element
cs[1].increment();   // ok
let v = cs[0].get(); // ok: ref self on boxed-array element (§11.3.1)

// box self example (requires box handle)
let bc = box(Counter(5));
bc.reset();          // ok: box self receiver
bc.increment();      // ok: ref mut self auto-borrows *bc (bc: box<Counter>)
let m = bc.get();    // ok: ref self; m == 1

let rc: robox<Counter> = bc;
// rc.increment();   // ERROR: ref mut self requires mutable place; robox is not mutable
// rc.reset();       // ERROR: reset requires box self, but rc is robox<Counter>
let k = rc.get();    // ok: ref self works on robox<Counter>
```

**Receiver capability comparison:**

| Receiver | Requires | Caller supplies | Can mutate fields | Escapes? |
|---|---|---|---|---|
| `self` | value `Self` | value (moved) | yes (owned copy) | — |
| `ref self` | addressable place or box/robox | mutable or immutable place | no | no |
| `ref mut self` | mutable place | mutable place only | yes | no |
| `box self` | `box<Self>` | box handle | yes | via box |

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

#### 12.3.1 Argument style restriction

**Mixing positional and named arguments is not allowed.** A call must use either all positional arguments or all named arguments.

```p7
// OK: all positional
Point(1, 2)

// OK: all named
Point(x = 1, y = 2)

// ERROR: mixed positional and named
Point(1, y = 2)
```

#### 12.3.2 Construction visibility restriction

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

Method receivers are defined in §11.4. Structs may use `self`, `ref self`, `ref mut self`, or `box self` receivers.

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
- Methods marked `@intrinsic()` are resolved at compile/codegen time; the runtime does **not** look up source implementations. The compiler lowers intrinsic method calls directly to intrinsic operations during codegen.

**Example: Builtin string with intrinsic method**

```p7
@builtin()
struct string {
  @intrinsic("string.len_bytes")
  pub fn len_bytes(self: ref<string>) -> int;
}
```

In the above example:
- `string` is a builtin nominal type with no source-level fields.
- `len_bytes` is a signature-only method marked as intrinsic.
- Calls to `s.len_bytes()` are resolved via normal method resolution, but code generation lowers the call directly to an intrinsic operation.
- No runtime source lookup occurs; intrinsics are fully resolved during compilation.

**Example: FFI handle**

```p7
@builtin()
struct FileHandle {
  @intrinsic()
  pub fn close(self) -> unit;
}
```

**Rationale:** `@builtin()` structs allow the compiler to define opaque types for fundamental types (like `string`), FFI, runtime handles, or platform-specific types without exposing internal representation. Intrinsic methods provide a discoverable API surface for IDE tooling (e.g., "Go to Definition") while maintaining the performance of compiler-lowered operations.

---

### 12.7 Representation attributes (FFI)

Protosept provides representation attributes for structs intended to cross FFI or host boundaries.

#### 12.7.1 `@repr(transparent)`

Applies only to **tuple structs with exactly one field**.

Rules:
- The struct MUST have exactly one field.
- The struct has the same size, alignment, and ABI passing convention as its single field.
- Using `@repr(transparent)` on any other struct form is ERROR.

Common use: newtype wrappers around FFI-safe scalars or pointers.

#### 12.7.2 `@repr(C)`

Applies to non-`@builtin()` structs that declare concrete fields.

Layout rules:
- Field order is preserved exactly as declared in source.
- Each field is placed at the next offset satisfying its alignment.
- Struct alignment is the max alignment of its fields.
- Total size is padded to a multiple of the struct alignment.

The compiler MUST compute and preserve layout metadata (`size`, `align`, and per-field `offset`) for `@repr(C)` structs in the compiled artifact when those structs appear in any `@ffi` signature (§19.6.1, §23.6).

Restrictions (v1):
- All fields of an `@repr(C)` struct MUST be FFI-safe types (§23.3). Otherwise ERROR.
- `@repr(C)` structs MAY be generic, but any `@ffi` call boundary MUST be monomorphic (§17.4, §23.4).

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

Method receivers are defined in §11.4. Enums may use `self`, `ref self`, `ref mut self`, or `box self` receivers.

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
- `throw` is a **contextual keyword**: it is permitted only in functions or closures with `throws` or `throws<E>` in their effect set (§11.2, §9.7).
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
- Arms use the same syntax as `match` arms (§9.6), including the full structural pattern grammar (§9.6.1).
- The thrown enum value is the scrutinee for pattern matching.
- Arms are tried in source order; the first matching arm’s expression becomes the result.

Syntax:
```p7
try expr else {
  pattern1 => expr1,
  pattern2 => expr2,
  _ => fallback_expr,
}
```

Example (unit variant patterns):
```p7
enum FileError(
  NotFound,
  PermissionDenied,
);

fn[throws<FileError>] read_file(path: string) -> string { ... }

fn safe_read(path: string) -> string {
  try read_file(path) else {
    FileError.NotFound        => "",
    FileError.PermissionDenied => "[access denied]",
  }
}
```

Example (payload variant patterns):
```p7
enum ParseError(
  InvalidToken: string,
  UnexpectedEof,
);

fn[throws<ParseError>] parse(input: string) -> int { ... }

fn safe_parse(input: string) -> string {
  try parse(input) else {
    ParseError.InvalidToken(tok) => f"bad token: {tok}",
    ParseError.UnexpectedEof     => "unexpected end of input",
  }
}
```

Arm patterns follow the same rules as `match` (§9.6.1), including the full structural pattern grammar. The thrown enum value is the scrutinee; all pattern forms supported in `match` are permitted here, including enum payload variant patterns.

Summary of available pattern forms (see §9.6.1.1 for the full grammar):
- **Wildcard**: `_` matches any thrown value.
- **Binding**: `name` binds `name` to the thrown value.
- **Path pattern**: `EnumName.VariantName` matches a specific unit enum variant.
- **Enum payload variant**: `EnumName.Variant(p1, ..., pn)` matches a payload variant and binds its fields.
- **Named binding (legacy)**: `name: pattern` binds `name` to the thrown value when the arm matches.

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

### 14.3 Calling callables with `throws` effect (explicitness rule)

If a call may throw (i.e., the callee — whether a named function or a closure — has `throws` or `throws<E>` in its effect set), the call MUST appear inside a `try` form. Bare calls are ERROR, even within functions or closures that themselves have a `throws` effect.

This rule applies uniformly to named function calls and closure calls (§9.7.4).

In a function or closure without a `throws` effect:
- only the handle form is allowed: `try call else ...`
- the propagate form is ERROR.

In a function or closure with a `throws` or `throws<E>` effect:
- either propagate or handle form is allowed.

### 14.4 Propagation compatibility rules for typed throws

When using the propagate form (`try expr`), the callee's throw effect must be compatible with the caller's throw effect:

| Callee effect | Caller effect | Propagation allowed? |
|---------------|---------------|----------------------|
| `throws<E>`   | `throws<E>`   | YES (exact match)    |
| `throws<E>`   | `throws<F>`   | ERROR (type mismatch)|
| `throws<E>`   | `throws`      | YES (untyped absorbs typed) |
| `throws`      | `throws<E>`   | ERROR (cannot narrow)|
| `throws`      | `throws`      | YES                  |

Rules:
- **Exact match**: A `throws<E>` callee may propagate in a `throws<E>` caller only when `E` is the same type.
- **Typed to untyped**: A `throws<E>` callee may propagate in an untyped `throws` caller. The typed exception becomes an untyped exception.
- **Untyped to typed forbidden**: A `throws` callee may NOT propagate in a `throws<E>` caller. The caller cannot guarantee that only `E` is thrown.

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
- `ref<T>` MUST NOT cross the boundary.
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

Protos are categorized as:
- **Object proto**: eligible for runtime dispatch via `box<P>` and `ref<P>`.
- **Static proto**: compile-time only; not eligible for runtime dispatch.

**Inference rule (object safety):**

A proto is an **object proto** iff all its methods are object-safe:
- `Self` MUST NOT appear in parameter types or return types except as the receiver.
- The receiver must be explicit: either `ref self` (shorthand for `self: ref<Self>`) or `box self` (shorthand for `self: box<Self>`).
- Generic methods in object protos are ERROR in v1.

Otherwise, the proto is a **static proto**.

**Built-in static protos:** `Copy`, `Send`.

**Using static protos in `box<P>` or `ref<P>` is ERROR.**

Examples:
```p7
// Object proto (all methods are object-safe)
proto Printable {
  fn print(ref self) -> unit;
}

// Static proto (method returns Self)
proto Clone {
  fn clone(ref self) -> Self;  // Self in return type
}

// ERROR: Cannot use static proto as `box<Clone>`
let x: box<Clone> = ...;  // ERROR
```

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

**Default implementations:**

Proto methods MAY have bodies (default implementations):

```p7
proto Describable {
  fn describe(ref self) -> string {
    return "Default description";
  }
}
```

When a type lists a proto in its conformance list (`struct[P, ...] ...` or `enum[P, ...] ...`), the default methods are injected if not already defined by the type.

**Injection rule:**
- Listing `P` in `struct[P, ...] ...` or `enum[P, ...] ...` injects default methods from `P` into the type if the type does not already define methods with the same signature.
- If multiple protos inject methods with the same signature and the type does not define that signature, it is an ERROR.
- If a type defines a method matching a proto method signature, the type's definition takes precedence (no injection for that method).

v1 restrictions:
- Proto methods MUST NOT mention `Self` in parameter or return types beyond the receiver (for object safety; see §18.2).
- Overloads in protos: ERROR in v1 (recommended).

### 18.4 Proto handles

There is no plain runtime value of proto type `P`.

Runtime proto handles are:
- `box<P>` – owned proto handle (§18.5)
- `ref<P>` – borrowed proto handle (§18.4.1)

Static protos MUST NOT appear as `box<P>` or `ref<P>`.

#### 18.4.1 Borrowed proto handles: `ref<P>`

**Well-formedness:**
- `ref<P>` is well-formed only when `P` is an object proto.
- Using `ref<P>` where `P` is a static proto is ERROR.

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

**Coercion rule:** If `P` is declared in `T`'s conformance list (via `struct[P, ...]` or `enum[P, ...]`), then implicit coercion is allowed at assignment, argument passing, and return sites (see §18.6). Otherwise, an explicit `as box<P>` cast is required.

#### 18.5.1 Converting `ref<T>` to `ref<P>` (borrowed upcast)

A `ref<T>` can be converted to `ref<P>` when `T` satisfies `P`.

**Explicit cast:**
```p7
let r: ref<SomeStruct> = ref(v);
let p: ref<Printable> = r as ref<Printable>;
```

**Coercion rule:** If `P` is declared in `T`'s conformance list (via `struct[P, ...]` or `enum[P, ...]`), then implicit coercion is allowed at assignment, argument passing, and return sites (see §18.6). Otherwise, an explicit `as ref<P>` cast is required.

Only `ref self` methods can be called on `ref<P>` (see §18.4.1).


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
- The compiler MUST check structural satisfaction after injection.
- Listing a proto enables implicit behaviors described by this spec:
  - Default method injection from the proto (§18.3).
  - `Copy` and `Send` opt-in behavior (§6.3, §6.5).
  - Implicit `box<T> -> box<P>` coercions for object protos (§18.5).
  - Implicit `ref<T> -> ref<P>` coercions for object protos (§18.5.1).

**Duplicate conformances:** Listing the same proto more than once in a struct's conformance list is a compile-time ERROR.

### 18.7 Dynamic dispatch

Calling a proto method on `box<P>` or `ref<P>` performs dynamic dispatch:
- The call dispatches to the implementation for the dynamic type of the underlying object.

**Receiver semantics:**

For `box<P>`:
- For methods with `ref self` receivers: the proto box handle is passed and dereferenced to obtain a `ref<T>` view of the boxed contents.
- For methods with `box self` receivers: the proto box handle itself is passed (as `box<P>`), aliasing the original box. The method receives a boxed handle, which satisfies `Copy`; multiple calls do not move the box.

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
b.mutate();  // dispatches to Counter.mutate; box handle satisfies Copy
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

### 19.1 Attachment sites
An attribute list may appear immediately before:
- `fn`, `struct`, `enum`

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

#### 19.6.1 FFI metadata (normative)

When the FFI extension is enabled by the host (§23.1), the compiled artifact MUST preserve enough metadata to support universal FFI call marshalling at runtime.

Minimum required metadata:
- For each function/method annotated with `@ffi(...)`:
  - resolved FFI key (§23.2.2)
  - ABI (§23.2.1)
  - full parameter and return type descriptions (after monomorphization)
- For each `@repr(C)` or `@repr(transparent)` struct that appears in any `@ffi` signature (directly or transitively):
  - representation kind (`C` or `transparent`)
  - `size` and `align`
  - per-field type and `offset` (for `@repr(C)`)

If required metadata is missing at runtime, invoking the corresponding `@ffi` function MUST TRAP with a descriptive error.

### 19.7 Errors
ERROR if:
- attribute name does not resolve to a `struct`
- unknown named field provided
- required field omitted
- non-constant value provided
- field type not permitted

### 19.8 Standard compiler attributes

The compiler recognizes certain standard attributes with special semantics:

#### 19.8.1 `@builtin()`
Marks a struct as a compiler-defined opaque type (§12.6).

#### 19.8.2 `@intrinsic()`
Marks a function or method as a compiler intrinsic.

Syntax:
```p7
@intrinsic()              // Intrinsic name derived from context
@intrinsic("name")        // Explicit intrinsic name
@intrinsic(name = "...")  // Named parameter form
```

**Semantics:**
- The function/method has no source implementation; it is a signature-only declaration.
- The compiler lowers calls to the intrinsic directly during codegen.
- No runtime source lookup occurs.
- Intrinsic names identify the specific compiler operation to use.

**Common use:**
- Methods on `@builtin()` structs (e.g., `string.len_bytes`)
- Compiler-provided operations that cannot be expressed in source

**Example:**
```p7
@builtin()
struct string {
  @intrinsic("string.len_bytes")
  pub fn len_bytes(self: ref<string>) -> int;
}
```

#### 19.8.3 `@host(...)`

Declares a function/method as implemented by the embedding host (not by Protosept source code).

Syntax:
```p7
@host(name = "qualified.host.name")
fn f(x: int) -> int;
```

Rules:
- A `@host` function/method is a signature-only declaration (no body). Providing a body is ERROR.
- The compiler MUST lower calls to a host-dispatched call using the provided `name`.
- If the host has not registered the named function at runtime, calling it MUST TRAP with message `host function not found: <name>`.

#### 19.8.4 `@ffi(...)`

Declares a function/method as implemented by a native symbol resolved via the host’s FFI facility.

Syntax:
```p7
@ffi(name = "puts", lib = "c", abi = "c")
fn puts(s: ptr) -> int;
```

Fields (v1):
- `name: string` (required) — native symbol name
- `lib: ?string = null` — library/module hint; `null` means “process-global / host-defined default”
- `abi: string = "c"` — calling convention; see §23.2.1

Rules:
- A `@ffi` function/method is a signature-only declaration (no body). Providing a body is ERROR.
- `@ffi` functions MUST be monomorphic at the call boundary; if the compiler cannot determine a monomorphic signature at the call site, it is an ERROR (§17.4, §23.4).
- Whether FFI is enabled is host-controlled (§23.1). If disabled, calling a `@ffi` function MUST TRAP.

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

Type parameter bounds use proto constraints. A bound list is written after `:` as one or more proto names separated by `+`:

```
type_param       := ident (':' proto_bound_list)?
proto_bound_list := proto_name ('+' proto_name)*
```

A single bound:
```p7
fn print_boxed<T: Printable>(value: box<T>) -> unit { value.print(); }
```

Multiple bounds:
```p7
fn dedup<T: Eq + Hash>(xs: array<T>) -> array<T> { ... }
```

#### Normative rules

1. **Conjunction**: `T: P + Q + R` requires the concrete type `T` to satisfy **all** listed protos. There is no disjunction, negation, or `where` clause in v1.
2. **Proto names only**: Each name in a bound list MUST resolve to a proto. Providing a non-proto type name is a compile-time ERROR.
3. **No duplicates**: Listing the same proto more than once for a single type parameter is a compile-time ERROR (e.g., `T: Eq + Hash + Eq` is ERROR).
4. **Order not significant**: `T: Eq + Hash` and `T: Hash + Eq` are semantically equivalent.
5. **At least one bound**: If `:` is present, at least one proto name MUST follow; a trailing `:` with no bound is ERROR.
6. **Usable members**: Inside the generic declaration body, `T` may use any members and operations made available by any of its declared bounds.

#### Examples

**Function — multiple bounds:**
```p7
fn dedup<T: Eq + Hash>(xs: array<T>) -> array<T> { ... }

fn render<T: Display + Eq>(x: T, y: T) -> string {
  if x == y { return "equal"; }
  return x.display();
}
```

**Struct — multiple bounds on a key type:**
```p7
struct HashMap<K: Eq + Hash, V>(...);
```

**Enum — multiple bounds:**
```p7
enum CacheResult<K: Eq + Hash, V>(
  Miss,
  Hit: V,
);
```

**Single bound (unchanged syntax):**
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

### 21.2 Functions and closures with `suspend` effect

A function or closure with the `suspend` effect (declared as `fn[suspend]` or `[suspend] (...) => ...`) may suspend via `yield;`.

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

// Closure with suspend effect
let task = [suspend] () => {
  yield;
};
```

Borrow restriction (v1):
- In a function or closure with the `suspend` effect, use of `ref<...>` is forbidden:
  - parameters of type `ref<T>` are ERROR
  - locals of type `ref<T>` are ERROR
  - `ref(x)` expression is ERROR
Rationale: avoids views living across suspension points without lifetime tracking.

Direct calling restriction (recommended):
- Callables with the `suspend` effect may be called directly only from within other callables that also have the `suspend` effect. [[TODO]] finalize.

### 21.3 `yield;`

`yield;` is a **contextual keyword**: in statement position within a function or closure with the `suspend` effect, it suspends the current fiber. Elsewhere, it may be used as an ordinary identifier.

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
- `f` MUST refer to a function or closure with the `suspend` effect.
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
- `f` is a (non-suspend) function or closure.
- All argument types MUST satisfy `Send`.
- If `f` is a closure, the closure itself MUST satisfy `Send` (i.e., all captured values must satisfy `Send`; §3.9).
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

## 23. Extension: FFI (native interop)

Status: optional extension.

FFI enables Protosept code to call native functions resolved by the host. Two resolution models are supported:
- **Dynamic resolver** (universal): library loading + symbol lookup at runtime.
- **Static registry** (universal): host-provided mapping from FFI key to function pointer/thunk.

### 23.1 Availability and capability

FFI is disabled by default unless explicitly enabled by the host.

When disabled:
- `@ffi(...)` declarations are allowed (for typechecking and tooling).
- Invoking a `@ffi` function MUST TRAP.

### 23.2 FFI calls

#### 23.2.1 ABI

Supported ABIs (v1):
- `"c"` — platform C ABI
- `"system"` — platform default system ABI (host-defined; optional)

Using an unsupported ABI string is ERROR.

#### 23.2.2 FFI key

For a declaration `@ffi(name = N, lib = L, ...)`, the **FFI key** is:
- If `lib` is `Some(L)`: `L + ":" + N`
- If `lib` is `null`: `N`

The compiled artifact MUST preserve the resolved FFI key (§19.6.1).

### 23.3 FFI-safe types (v1)

FFI-safe types are types that may appear in `@ffi` function signatures.

FFI-safe (v1):
- Scalars:
  - Core primitives: `int`, `float`, `bool`, `unit`, `ptr`
  - Fixed-width FFI scalars from `std.ffi` (§23.3.1)
- `@repr(transparent)` tuple structs whose single field type is FFI-safe
- `@repr(C)` structs whose fields are all FFI-safe (recursively). These are C POD structs.

Not FFI-safe (v1):
- `string`, `array<T>`, tuples `(T1, ...)`, `box<T>`, `robox<T>`, `ref<T>`, `proto`, `?T` (except `?ptr`)
- any struct/enum without an explicit FFI representation (`@repr(C)` or `@repr(transparent)`)

Using a non-FFI-safe type in a `@ffi` signature is ERROR.

#### 23.3.1 `std.ffi` fixed-width scalar types (normative)

To support C POD structs and universal FFI marshalling without expanding the core language primitive set, fixed-width scalar types are provided by the host in the `std.ffi` module when the FFI extension is enabled.

When §23 is enabled, the host MUST make module `std.ffi` available for import (even in `nostd` mode).

`std.ffi` defines the following nominal scalar types with fixed, platform-independent widths:
- `i8`, `i16`, `i32`, `i64` — signed two's-complement integers of the given bit width
- `u8`, `u16`, `u32`, `u64` — unsigned integers of the given bit width
- `isize`, `usize` — signed/unsigned pointer-sized integers
- `f32` — IEEE-754 binary32
- `f64` — IEEE-754 binary64

These types are FFI-safe and may appear in `@repr(C)` structs and `@ffi` signatures. The compiled artifact MUST preserve their exact size/alignment as part of signature/layout metadata (§19.6.1).

Note: The core `int` and `float` types remain `i64` and `f64` respectively; `std.ffi` types exist specifically to express C ABI widths.

### 23.4 Monomorphization requirement

Generics are compile-time only (§20). For FFI:
- Every `@ffi` call boundary MUST be monomorphic.
- Exported/host-visible entrypoints that are `@ffi` MUST be monomorphic.

### 23.5 Resolution models (host responsibility)

Hosts MAY implement one or both resolution models.

**(A) Dynamic resolver (universal)**
- Given the `lib` hint (if any) and `name` (or FFI key), open the library and resolve the symbol at runtime.
- The resolver SHOULD cache library handles and symbol addresses (host-defined policy).

**(B) Static registry (universal)**
- Host provides a registry mapping FFI key → callable function pointer or thunk.

### 23.6 Universal call marshalling (normative)

When invoking an `@ffi` function, the runtime MUST:
- use the recorded monomorphic signature and representation metadata (§19.6.1, §12.7)
- marshal arguments according to the selected ABI (§23.2.1) and type layouts
- call the resolved native symbol
- marshal the return value back into a Protosept value

`@repr(C)` POD structs MAY be passed **by value** and returned **by value** in v1.

If marshalling cannot be performed (unsupported ABI, missing layout metadata, unsupported type), the call MUST TRAP with a descriptive error.

---

## 24. Open items / TODO list (curated)

1) String concatenation spelling, slicing APIs
2) Boxed array mutation API surface: specify `push`, `pop`, `set`, `insert`, `remove` signatures (§3.3.3)
3) Enablement mechanisms for extensions (§21, §22)
4) Host ABI: concrete API surfaces for calling, fibers, threads (§17, §21.4, §22)
5) Specify prelude location/definition of `box<T>.new` intrinsic method (§7.4)
6) Specify representation/ABI attributes for FFI: `@repr(transparent)` and `@repr(C)`; define compiled layout metadata requirements (§12.7, §19.6.1, §23)
7) Define universal marshalling surface for strings/arrays/callbacks under FFI (beyond POD + ptr) (§23)
8) Closure parameter type inference from expected type (§9.7.2)
9) Explicit capture list syntax for closures (§9.7.3)
10) Function reference expressions — obtaining a value of function type from a named function declaration (§3.9)

---
End.
