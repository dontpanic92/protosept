# p7 Language Specification (Draft)

Status: Draft  
Design goals (north star):
- **Statically typed scripting**: concise, readable, low ceremony.
- **Limited syntax/grammar**: features must pay rent in simplicity and interop.
- **Correctness by default**: explicit nullability, explicit borrowing, explicit identity/mutation.
- **Host interop**: easy to embed; predictable runtime values and errors.

This document defines the *intended* semantics. Where implementation details are not finalized, sections use `[[TODO]]`.

---

## 0. Notation

- `T, U, V` are types.
- `x, y, z` are identifiers.
- `null` denotes the null value (only inhabits nullable types).
- “Slot” means a storage location introduced by `let`/parameters.

---

## 1. Program structure

A program is a sequence of top-level items:

- Function declarations: `fn ...`
- Struct declarations: `struct ...`
- Enum declarations: `enum ...`
- Proto declarations: `proto ...`

[[TODO]]: module/import system and visibility rules at module boundaries.

Top-level executable statements are not allowed in v1; execution begins by calling an entrypoint function via host embedding (e.g., `run_p7_code(contents, "main")`).

---

## 2. Lexical structure

### 2.1 Identifiers
Identifiers start with `_` or a letter and continue with letters, digits, or `_`.

### 2.2 Keywords (reserved)
`fn`, `struct`, `enum`, `proto`, `let`, `pub`, `return`, `if`, `else`, `throw`, `try`, `loop`, `break`, `continue`

[[TODO]]: confirm final keyword set; keep minimal.

### 2.3 Comments
- Line comments: `// ...`
- Block comments: `/* ... */`

---

## 3. Types

### 3.1 Primitive types
p7 provides the following primitive types:

- `int`  
  Signed integer. [[TODO]]: width (recommend `i64` for scripting).

- `float`  
  IEEE floating point. [[TODO]]: width (recommend `f64`).

- `bool`  
  Boolean. Values: `true`, `false`.  
  [[TODO]]: confirm literals exist as keywords or identifiers.

- `unit`  
  The unit type, representing “no value”. The only value is `()` [[TODO]] or implicit.

### 3.2 String type
- `string` is a built-in **immutable value type** representing textual data.
- Copy/move semantics are defined in §6.
- `string` may internally share storage (e.g. copy-on-write), but this is not semantically observable.

[[TODO]]: encoding (UTF-8 recommended), indexing semantics (bytes vs codepoints).

### 3.3 Array type
- `array<T>` is a built-in **immutable value type** representing a sequence of `T`.
- Value arrays cannot be mutated in place.
- In-place mutation and identity/aliasing are provided by `box<array<T>>` (§3.6, §7.4).

[[TODO]]: surface syntax for array literals, indexing, length, iteration.
[[TODO]]: define boxed array mutation APIs (e.g., push/pop/set) and their signatures.

### 3.4 Nullable types
- `?T` is a nullable type: value is either `null` or a non-null `T`.

This aligns with a generic spelling `nullable<T>`:
- `?T` is syntactic sugar for `nullable<T>`.

Rules:
- `null` is only assignable to `?T`.
- `T` is not implicitly convertible to `?T` unless explicitly wrapped/promoted by a rule [[TODO]].
- Unwrapping rules are in §9.

Examples:
- `let x: ?int = null;`
- `let y: int = x;`  // error unless proven non-null via control flow [[TODO]]

### 3.5 Borrow (reference view) type: `&T`
p7 has **borrowed reference views**:

- `&T` : a **read-only** borrowed view of an existing slot/sub-location holding a `T`.

Borrowed views are **non-escapable** (§7).

Views compose naturally with nullability:
- `&?T` is a view of a nullable slot/value of type `?T`.

> Important: `&T` is *not* a heap box and is *not* an owned reference.

There is **no** `&mut T` in v1. All shared mutation and escaping references are done via `box<T>` (§3.6, §7.4).

### 3.6 Owned heap (box) type: `box<T>`
`box<T>` is an **owned heap-allocated container** that stores a `T` and provides **stable identity** and **shared, escapable reference-like semantics**.

Intuition:
- `T` is a value.
- `box<T>` is a reference-like handle to a heap cell containing a `T`.

Properties:
- `box<T>` values **can escape** (stored in structs/arrays, returned, captured, interop).
- Copying a `box<T>` copies the handle (aliases the same boxed cell).
- Mutation of the boxed contents is visible through all aliases (shared mutation).

[[TODO]]: exact surface syntax for boxing; `box(expr)` is recommended.

### 3.7 User-defined types
- `struct Name(...) { ... }` defines a nominal product type with fields (tuple-like) and an optional method block.
- `enum Name { ... }` defines a nominal sum type.
- `proto Name { ... }` defines a structural interface for dynamic dispatch (§12).

No inheritance.

---

## 4. Values and literals

### 4.1 Integer literals
Decimal digits, with optional `_` separators.  
Examples: `0`, `42`, `1_000_000`

### 4.2 Float literals
Decimal with a `.`; optional `_` separators.  
Examples: `1.0`, `3.1415`, `1_000.5`

[[TODO]]: exponent notation.

### 4.3 String literals
Double-quoted strings: `"hello"`  
[[TODO]]: escapes.

### 4.4 Boolean literals
`true`, `false` [[TODO]].

### 4.5 Null literal
`null` literal exists and has type `?T` only when context supplies `T`.  
Examples:
- `let x: ?int = null;`
- `return null;` only valid when function returns `?T`.

---

## 5. Bindings (slots), shadowing, and rebind

### 5.1 Slots and `let`
`let x = expr;` introduces an immutable slot (single-assignment binding).

Reassignment to an existing slot is not supported in v1:
- `x = expr` is always a compile-time error.

### 5.2 Shadowing (rebind)
A `let` may introduce a new binding with the same name as an existing binding in an outer scope:

```p7
let a = 1;
{
  let a = 2;  // shadows outer a
  a           // 2
}
a             // 1
```

This is **shadowing**, not mutation:
- the outer binding is not modified
- the new binding is visible only within its scope

Type rule (v1):
- If a name is shadowed, the new binding must have the **same type** as the shadowed binding (after inference), unless an explicit type annotation is provided on the new binding. [[TODO]] finalize whether explicit annotation may change the type.

Rationale: keep shadowing predictable and avoid turning it into an untyped “variable reuse” mechanism.

### 5.3 Identity and mutation
Mutation in v1 is performed only through `box<T>`:
- field assignment on `box<Struct>` (e.g. `p.x = 1` where `p: box<Point>`)
- boxed cell update operations (e.g. `*b = value`) [[TODO]] exact syntax
- in-place mutation operations on boxed arrays (e.g. `a.push(x)` where `a: box<array<T>>`) [[TODO]] exact API

There is no direct in-place mutation of value structs:
- `s.x = 1` is illegal when `s: Struct` (non-box).

There is no direct in-place mutation of value arrays:
- in-place update of `a[i] = v` is illegal when `a: array<T>` (non-box) [[TODO]] (pending final indexing syntax).

---

## 6. Moves and copies (core rule set)

### 6.1 Move-by-default
For a value of type `T`:
- If `T` is not `Copy`, then:
  - `let b = a;` **moves** `a` into `b`.
  - After move, `a` is invalid to use (compile-time “moved” error).
- If `T` is `Copy`, then:
  - `let b = a;` **copies**.

The same rule applies to:
- passing arguments to parameters of type `T`
- returning values of type `T`

### 6.2 The `Copy` marker
Types may opt into `Copy`. `Copy` indicates *implicit duplication* is allowed.

Copy is **structural**:
- Copying a primitive duplicates the bits/value.
- Copying a struct duplicates each field (using each field’s copy rule).
- Copying an array duplicates its storage and copies each element.  
  Note: `array<T>` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are “as if” a value copy occurred.
- Copying a string duplicates its storage and copies its bytes/code units.  
  Note: `string` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are “as if” a value copy occurred.

Copying handles:
- Copying a `box<T>` copies the handle (aliases the same boxed cell). `box<T>` is `Copy` by default (recommended).

Eligibility:
- A struct may be `Copy` only if all fields are `Copy` and the struct explicitly opts in.
- `array<T>` is `Copy` iff `T` is `Copy` [[TODO]] (recommended: yes).
- `string` is `Copy` iff it opts in; recommended: allow (immutable value semantics; implementation may share storage).

[[TODO]]: Syntax for opting into Copy: e.g. `struct A [Copy](...) { ... }` or similar.

### 6.3 Explicit copying and cloning
p7 may provide explicit operations:
- `copy(x)` : requires `T: Copy`, returns a copied value
- `clone(x)` : requires `T: Clone`, returns a duplicated value

[[TODO]]: Whether `Clone` exists in v1; recommended: postpone until needed.

### 6.4 Drop / destruction
[[TODO]]: whether p7 exposes deterministic destructors. Likely **no** in v1 (GC-based runtime).

---

## 7. Borrowed views (`&T`) and boxes (`box<T>`)

### 7.1 Meaning of `&T` (read-only view)
A borrowed view refers to an **existing storage location** (slot or sub-location).

If `r: &T` refers to `x: T`:
- `*r` reads the current value of `x`.

### 7.2 Taking views
- `&x` is allowed when `x` is addressable (slot or sub-location).

[[TODO]]: whether `&` can be taken of temporaries (recommended: no in v1).

### 7.3 Non-escapable rule (hard rule in v1)
Values of type `&T` **must not escape** their scope.

A view value cannot be:
- returned from a function
- assigned into a struct field
- assigned into an array element
- stored in any heap-allocated value (including `box<...>`)
- stored in globals/statics
- captured by closures (if/when closures exist)
- passed to host interop boundaries as a persistent value [[TODO]] (viewing may be supported only during a call)

Consequences:
- user-defined types cannot contain fields of type `&...`
- arrays cannot contain `&...` elements

This avoids needing escape analysis or lifetime tracking in v1.

### 7.4 Meaning of `box<T>`
A `box<T>` contains a `T` and provides:
- stable identity
- escapable storage
- shared mutation (mutation through a box is visible through all aliases)

Operations (surface syntax TBD):
- Construction: `box(expr)` allocates a new boxed cell containing `expr`.
- Read/deref: `*b` reads the inner `T` (by move or copy depending on `T`) [[TODO]].
- Write/set: `*b = expr` writes a new `T` into the cell [[TODO]].
- Member access auto-deref: `b.field` and `b.method(...)` access the inner value. [[TODO]] (recommended: yes).
- Field assignment: `b.field = expr` updates the inner struct field **in-place** (only valid when `b: box<S>`).
  - This is a direct interior update of the boxed cell’s contents, not a desugaring to read-modify-write of `S`.

[[TODO]]: define the precise semantics of `*box` read/write and member auto-deref, including rules for reading/moving non-`Copy` values out of a box.

---

## 8. Expressions

### 8.1 Expression categories
Expressions include:
- literals
- identifiers
- unary operations
- binary operations
- function calls
- field access
- block expressions
- `if` expressions
- loop expressions (`loop ...`) (§8.5)
- `try` expressions (error handling)

### 8.2 Block expressions
A block `{ ... }` contains a sequence of statements.

Value of a block:
- If the final statement is an expression statement without a trailing semicolon, the block evaluates to that expression’s value.
- Otherwise the block evaluates to `unit`.

### 8.3 `if` expression
`if condition then_expr else else_expr`

- `condition` must be `bool`.
- `then_expr` and `else_expr` must have compatible types.
- The `if` expression’s type is the common type (or requires explicit conversions) [[TODO]].

### 8.4 Operators and precedence
[[TODO]]: specify full operator set and precedence table.

### 8.5 Loop expressions
`loop` is an expression that repeats execution of a body until a `break` is executed (or a `throw` escapes). A `loop` may yield a value via `break value`.

#### 8.5.1 Forms
Two forms exist:

1) Infinite loop:
```p7
loop {
  body
}
```

2) Loop with header bindings (init + step):
```p7
loop (init; step) {
  body
}
```

Where:
- `init` is exactly **one** `let` binding evaluated once, before the first iteration.
- `step` is exactly **one** `let` binding evaluated after each iteration that completes normally (i.e. not via `break`).
- The `step` binding must bind the **same name** as `init`.

Grammar sketch:
- `init := let name = expr`
- `step := let name = expr`  (same `name` as `init`)

Example:
```p7
let out = loop (let i = 0; let i = i + 1) {
  if i > 10 { break i; }
};
```

To carry multiple pieces of state, use a single state value (struct or tuple):

```p7
struct State(i: int, sum: int);

let sum = loop (let s = State(0, 0); let s = State(s.i + 1, s.sum + s.i)) {
  if s.i > 10 { break s.sum; }
};
```

#### 8.5.2 Scope and visibility of loop bindings
A `loop (init; step) { body }` introduces a **loop scope**.

- The binding introduced by `init` is defined in the loop scope and is visible in `body` and `step`.
- The binding introduced by `step` becomes the binding for the next iteration in the loop scope.

Outer bindings with the same name may exist; they are **shadowed** inside the loop scope and are unaffected by the loop.

Example:
```p7
let i = 0;
let out = loop (let i = 1; let i = i + 1) {
  if i > 3 { break i; }
};
// out == 4
// outer i is still 0
```

#### 8.5.3 Step evaluation
Because `step` is a single `let` binding, its right-hand side is evaluated using the binding from the current iteration, and the new binding becomes visible at the start of the next iteration.

#### 8.5.4 Control flow: `break` and `continue`
Inside a loop body:

- `break;` exits the loop and yields `unit`.
- `break expr;` exits the loop and yields the value of `expr`.

- `continue;` ends the current iteration early and proceeds to the next iteration:
  - it executes the `step` clause (if present) and then begins the next iteration
  - in `loop { ... }` without header, it simply begins the next iteration

Type of a `loop` expression:
- If a loop contains any `break expr;`, the loop expression's type is the common type of all break values.
- If the loop uses only `break;`, the loop expression has type `unit`.

[[TODO]]: define the exact “common type” rules (must be identical type in v1 recommended).

#### 8.5.5 Normal completion
A loop expression does not complete normally by reaching the end of its body; it runs until a `break` is executed, or until it throws.

#### 8.5.6 `break` and `step` interaction
- `break` does **not** execute the `step` clause.
- `continue` **does** execute the `step` clause.

#### 8.5.7 Interaction with `&T` views
Because shadowing creates new bindings (new slots), a view `&x` taken in one iteration refers to that iteration’s binding and must not escape (§7). Views cannot be stored for use across iterations.

### 8.6 `try` expressions
See §14.

---

## 9. Statements

### 9.1 Statement forms
- `let` binding: `let x = expr;`
- expression statement: `expr;`
- `return expr;` or `return;` (returns `unit`)
- `throw expr;`
- `break;` and `break expr;` (only valid inside `loop`)
- `continue;` (only valid inside `loop`)
- declarations (functions/types) [[TODO]] where allowed

### 9.2 Return semantics
Functions return the value of:
- an explicit `return`, or
- the last expression of the function body block (if not terminated by `;`), otherwise `unit`.

[[TODO]]: decide whether implicit return is allowed for all functions; recommended yes (script-friendly).

---

## 10. Functions

### 10.1 Function declaration
Form:
```p7
fn name(param1: T1, param2: T2, ...) -> R {
  ...
}
```

- Return type `-> R` may be optional; if omitted, default is `unit`. [[TODO]] confirm.
- Parameters are slots local to the function body.

### 10.2 Parameter passing
For parameter type `T`:
- argument passing follows move-by-default/copy rules (§6).

For parameter type `&T`:
- caller must pass an addressable location and use explicit `&` at the call site.
- no implicit borrowing in v1.

Mutating inputs requires `box<T>` parameters (including `box<array<T>>` for arrays).

### 10.3 Named arguments and defaults
p7 supports:
- named arguments: `f(x = 1, y = 2)`
- default values in parameter declarations: `fn f(x: int = 1) { ... }`

Rules:
- Calls may be positional or named.
- Mixing named and positional arguments in the same call is [[TODO]] (recommended: disallow).
- Default argument expressions are evaluated at call time [[TODO]].

---

## 11. Structs

### 11.1 Declaration (tuple-like only)
Struct fields are declared in tuple-like form. There is **no block-like field declaration syntax**.

Form:
```p7
struct Point(
  x: int,
  y: int,
);
```

Fields may have:
- `pub` visibility modifier [[TODO]] for module system
- default value: `x: int = 0`

### 11.2 Optional method block
A struct may be followed by a method block containing only method declarations:

```p7
struct Vec2(
  pub x: float = 0,
  pub y: float = 0,
) {
  pub fn length(&self) -> float {
    // ...
  }
}
```

Receivers in v1:
- `self` (by value; move/copy)
- `&self` (read-only view)

There is no `&mut self` in v1. In-place mutation APIs should use `box<Self>` parameters (or be expressed as free functions taking `box<T>`).

### 11.3 Construction
Struct values are constructed by calling the struct name:
- `Point(1, 2)`
- with named args: `Point(y = 2, x = 1)`

[[TODO]]: whether a `Self(...)` expression exists inside methods.

### 11.4 Field access and assignment
- Access: `s.x` is allowed on a struct value `s: S` (read-only).
- Assignment: `s.x = v` is **illegal** unless `s` is a `box<S>`.

Example:
```p7
let p = box(Point(1, 2));
p.x = 10; // ok
```

---

## 12. Protos (structural polymorphism and dynamic dispatch)

### 12.1 Overview
A `proto` defines a **structural interface**: a set of required method signatures.

A concrete type `T` implements a proto `P` if `T` provides methods matching every required signature in `P`.

Proto values are **boxed-only**:
- The only way to hold a dynamic-dispatch value of proto type `P` is via `box<P>`.
- There is no plain value of type `P`.

Rationale:
- keeps dispatch and ownership uniform
- avoids hidden boxing
- makes sharing/escaping explicit

### 12.2 Proto declaration
Form:
```p7
proto Printable {
  fn print(&self) -> unit;
}
```

Rules:
- Method name must match exactly.
- Parameter types and return type must match exactly.
- Receiver must be `&self` in v1.

Restrictions in v1:
- Proto methods must use `&self` receiver only.
- Proto methods must not mention `Self` as a type (in parameters or return types). [[TODO]] may be added later.
- Overloads in proto are [[TODO]] (recommended: disallow in v1).

### 12.3 Converting a concrete box to a proto box
A value of type `box<T>` may be converted to `box<P>` only with an explicit conversion, and only if `T` implements `P`.

Example (cast syntax TBD):
```p7
let v = box(Vec2(1, 2));
let p: box<Printable> = v as Printable;   // [[TODO]] exact syntax
p.print(); // dynamic dispatch
```

Semantics:
- The conversion does not allocate a new `T`; it reinterprets the existing box handle with an associated dispatch table for `P`.
- If `T` does not implement `P`, the conversion is a compile-time error.

[[TODO]]: decide cast spelling:
- `v as Printable` (where `Printable` is a proto)
- or `v as box<Printable>`
- or `to_proto<Printable>(v)`

### 12.4 Dynamic dispatch
Calling a proto method on `box<P>` performs dynamic dispatch:
- `p.print()` invokes the concrete implementation for the dynamic type stored in `p`.

### 12.5 Optional nominal conformance assertions (compiler hints)
A struct declaration may include proto names as annotations to request an explicit compile-time conformance check:

```p7
struct[Printable] Vec2(
  x: float,
  y: float,
) {
  pub fn print(&self) -> unit { ... }
}
```

Meaning:
- `Vec2` must implement `Printable` structurally, otherwise compilation fails with a method mismatch error.
- This does not change runtime dispatch or conversion rules; it is an assertion/documentation feature.

[[TODO]]: whether `struct[...]` annotations are limited to proto names in v1 (recommended: yes).

### 12.6 Downcasting / type tests
[[TODO]]: Provide runtime type tests and downcasts for proto boxes, e.g.:
- `p is Vec2`
- `p as Vec2` returning `?box<Vec2>` or throwing on failure

### 12.7 Nullability
- `?box<P>` is the nullable proto-handle type.
- `box<P>` itself is non-null.

---

## 13. Enums

### 13.1 Declaration
```p7
enum SomeErrors {
  NumberIsNot42,
  [[TODO]]: payload variants? e.g. Number(i:int)
}
```

v1 may support only unit variants (names only). Payload variants are [[TODO]].

### 13.2 Values and namespacing
Enum variants are referenced as:
- `SomeErrors.NumberIsNot42`

[[TODO]]: whether variants are in scope unqualified.

---

## 14. Error handling (`throw`, `try`)

### 14.1 Throwing
`throw expr;` aborts evaluation and transfers control to the nearest enclosing `try`.

[[TODO]]: whether thrown values must be of an enum type or any value is allowed.

### 14.2 Try expressions
Form:
- `try expr else fallback_expr`
- or a match-like else block:
```p7
let v = try f() else {
  err: SomeErrors.NumberIsNot42 => 0,
  _ => 1,
};
```

Rules:
- `try` is an expression.
- If `expr` completes normally, its value is the result of the `try`.
- If `expr` throws, the `else` branch is evaluated.
- Pattern syntax and matching rules are [[TODO]].
- Type of the `try` expression is the common type of normal result and else result.

[[TODO]]: represent error types and whether “throws” is typed/checked. (Current direction: no user-defined effects in v1.)

---

## 15. Standard conversions and type checking

### 15.1 Numeric coercions
[[TODO]]: decide numeric coercions.
Recommendation for scripting:
- allow implicit `int -> float` promotion in arithmetic/comparison
- require explicit conversion elsewhere

### 15.2 Nullability checks
Rules for using `?T`:
- You cannot use a `?T` where `T` is required unless:
  - you check for non-null in control flow and the compiler narrows the type, or
  - you use an explicit unwrap operator `!` [[TODO]].
- Provide `??` operator: `x ?? default` [[TODO]].

---

## 16. Memory model / runtime model (informative)

p7 uses a GC-based runtime. However, the language semantics are defined in terms of:
- value moves/copies
- borrowed views that alias slots (non-escapable)
- boxed identity containers (`box<T>`) that can escape and can be mutated

Implementation may represent values on stack or heap; this is not semantically observable.

[[TODO]]: specify runtime value set for host interop:
- int/float/bool/unit/null
- string
- array
- struct instances (values)
- box instances (handles)
- enum values
- proto dispatch tables / type ids as needed
- (no borrowed views as persistent values)

---

## 17. Host interop (v1 requirements)

### 17.1 Calling into p7
Host calls a named p7 function with arguments and receives either:
- a returned value, or
- an error/exception object if `throw` escapes.

[[TODO]]: define API and error mapping.

### 17.2 Calling host functions from p7
Host may register functions callable by p7.

Requirements:
- Interop supports `?T` mapping to/from host null.
- Borrowed views (`&T`) do not cross the boundary as persistent values.
  They may be passed to host only for the duration of a call, or disallowed entirely in v1 [[TODO]].
- Boxes (`box<T>`) are the primary mechanism for passing identity/mutable objects across the boundary.
- Proto boxes (`box<P>`) are the primary mechanism for passing dynamically-dispatched objects across the boundary.

### 17.3 Ownership rules
- Passing a value type `T` to host follows move/copy semantics.
- Boxes are handles; passing `box<T>` copies/moves the handle per rules in §6.

---

## 18. Summary of chosen decisions (from discussion)

- Move-by-default (§6.1).
- Nullability uses `?T` prefix (sugar for `nullable<T>`) (§3.4).
- `&T` exists as read-only non-escapable view; no `&mut` (§3.5, §7).
- Shared mutation and escaping references use `box<T>` (§3.6, §7.4).
- Reassignment is not supported; `let` is single-assignment (§5.1).
- Shadowing (“rebind”) is supported: `let a = ...; let a = ...;` in inner scopes (§5.2).
- `loop` is an expression and supports `break value` (§8.5).
- `loop (init; step)` uses exactly one `let` in init and step, and `step` must bind the same name as `init` (§8.5.1).
- Field assignment is allowed only through `box<Struct>` (§5.3, §7.4, §11.4).
- Arrays and strings are **immutable value types**; in-place mutation requires boxing (e.g. `box<array<T>>`) (§3.2, §3.3, §5.3).
- `box<T>` mutation is **in-place** and visible through all aliases (§7.4).
- Protos are structural and boxed-only (`box<P>`) (§12).
- Structs are tuple-like only for fields; blocks are only for methods (§11).

---

## 19. Open items / TODO list

1) Finalize integer/float widths
2) Decide if `string` is `Copy` by default (copy may share storage internally)
3) Decide `copy(x)`/`clone(x)` existence and naming
4) Define arrays: literal syntax, indexing, bounds behavior
5) Define boxed array mutation APIs and semantics
6) Define string: encoding, indexing, slicing
7) Define enum payload variants (if any)
8) Define error model: thrown value types, matching/patterns
9) Define module system & visibility
10) Define host ABI/value representation and ownership transfer
11) Finalize proto cast syntax (`box<T>` -> `box<P>`) and runtime dispatch table caching
12) Define precise semantics of `*box` read/write and member auto-deref (including reads of non-`Copy` from a box)
13) Finalize shadowing type rule (whether explicit annotation may change type)
14) Finalize whether `&` can be taken of temporaries

---
End.
