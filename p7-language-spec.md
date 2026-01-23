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
`fn`, `struct`, `enum`, `proto`, `let`, `pub`, `return`, `if`, `else`, `throw`, `try`, `loop`, `break`, `continue`, `yield`

[[TODO]]: confirm final keyword set; keep minimal.
Note: `yield` is reserved even though it is only valid when the Fiber extension is enabled (§20).

### 2.3 Comments
- Line comments: `// ...`
- Block comments: `/* ... */`

---

## 3. Types

### 3.1 Primitive types
p7 provides the following primitive types:

- `int`  
  Signed 64-bit two’s-complement integer (i64).  
  **Overflow**: traps (runtime error) in v1 (§15.1).

- `float`  
  IEEE-754 binary64 floating-point (f64).  
  [[TODO]]: specify NaN/Inf behavior details and conversions.

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
- If a name is shadowed, the new binding must have the **same type** as the shadowed binding (after inference), unless an explicit type annotation is provided on the new binding. [[TODO]] finalize whether explicit annotation may change type.

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

### 6.2 Conformances (`struct[...]`) and implicit behavior
A struct declaration may include a bracket list of **conformances**:

```p7
struct[Conformance1, Conformance2] Name(
  ...
) {
  ...
}
```

A conformance name in `struct[...]` is one of:
- a **proto name** (e.g. `Printable`), or
- a **built-in marker conformance** (currently: `Copy`).

For each conformance listed, the compiler:
1) Performs a compile-time conformance check (structural).
2) Enables certain *implicit* behaviors (coercions / operations) associated with that conformance.

If a struct lists a conformance that it does not satisfy, compilation fails.

Notes:
- `struct[...]` does **not** inject methods or fields into the type. It only checks and enables implicit behaviors described in this specification.
- The bracket list is about *implicitness*. A program may still use explicit casts/operations based on structural properties (see §12.3 and §6.4).

[[TODO]]: precise grammar for the bracket list, including whether duplicates are allowed (recommended: disallow) and whether order matters (recommended: no).

### 6.3 The `Copy` marker conformance
`Copy` indicates that *implicit duplication* is allowed for a type.

Copy is **structural**: a type is **Copy-eligible** if it can be duplicated by duplicating its parts (per the rules below). However, a type is treated as `Copy` (i.e. participates in implicit copy behavior) only if it:
- is Copy-eligible, and
- declares `Copy` in `struct[...]`.

In other words:
- Copy-eligible is a structural property.
- Declaring `[Copy]` opts a struct into implicit copy behavior.

#### 6.3.1 Copy behavior
When a value of type `T` is copied:
- Primitives: duplicating a primitive duplicates the bits/value.
- Structs: copying duplicates each field (using each field’s copy rule).
- Arrays: copying duplicates the array value and copies each element.
  - `array<T>` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are “as if” a value copy occurred.
- Strings: copying duplicates the string value.
  - `string` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are “as if” a value copy occurred.
- Boxes: copying a `box<T>` copies the handle (aliases the same boxed cell). This is a shallow copy of the handle, not a deep copy of `T`.

#### 6.3.2 Copy-eligibility
- `int`, `float`, `bool`, `unit` are Copy-eligible.
- `box<T>` is Copy-eligible (handle copy) and is `Copy` by default.
- `?T` is Copy-eligible iff `T` is Copy-eligible.
- `&T` is not `Copy` (and views are non-escapable regardless; §7.3).
- `array<T>` is Copy-eligible iff `T` is Copy-eligible. [[TODO]] confirm this choice.
- `string` is `Copy` by default (immutable value semantics; may share storage internally).

User-defined structs:
- A struct `S(...)` is Copy-eligible iff all of its field types are Copy-eligible.
- A struct is treated as `Copy` (i.e. copies implicitly) only if it declares `[Copy]`.

Policy choices:
- `array<T>` default: if `array<T>` is Copy-eligible, it may be `Copy` by default or require explicit opt-in; [[TODO]] decide. (If defaulting to Copy, prefer doing so only when `T` is Copy-eligible.)

### 6.4 Explicit copying
p7 provides an explicit copying operation:

- `copy(x)` : requires that `T` is Copy-eligible, and returns a copied value of type `T`.

This operation may be used even if `T` does not declare `[Copy]`.
Rationale: structural properties may be used explicitly, but implicit duplication must be opted into by declaring `[Copy]`.

### 6.5 Clone
[[TODO]]: Whether `Clone` exists in v1; recommended: postpone until needed.

### 6.6 Drop / destruction
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

Note: `yield` is a statement/expression only under the Fiber extension (§20); it is not part of core expressions in v1.

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
- `throw expr;` (only valid in `fn[throws]` / `fn[throws<E>]`; §14)
- `break;` and `break expr;` (only valid inside `loop`)
- `continue;` (only valid inside `loop`)
- `yield;` (only valid in `fn[fiber]` when Fiber extension is enabled; §20)
- declarations (functions/types) [[TODO]] where allowed

### 9.2 Return semantics
Functions return the value of:
- an explicit `return`, or
- the last expression of the function body block (if not terminated by `;`), otherwise `unit`.

[[TODO]]: decide whether implicit return is allowed for all functions; recommended yes (script-friendly).

---

## 10. Functions

### 10.1 Function declaration (core)
Core form:
```p7
fn name(param1: T1, param2: T2, ...) -> R {
  ...
}
```

- Return type `-> R` may be optional; if omitted, default is `unit`.
- Parameters are slots local to the function body.

### 10.2 Function signature qualifiers (`fn[...]`)
A function declaration may include an optional bracket list of **signature qualifiers**:

```p7
fn[qual1, qual2, ...] name(params...) -> R { ... }
```

Signature qualifiers are **part of the function's type and calling contract**. They are not general-purpose attributes.

Rules:
- Order does not matter.
- Duplicates are disallowed. [[TODO]]: specify diagnostics.
- In v1, the set of allowed qualifiers is closed and limited to:
  - `throws` and `throws<E>`
  - `fiber` (Fiber extension; §20)

[[TODO]]: finalize grammar for qualifiers including whether spaces are permitted in `throws<E>`.

### 10.3 Parameter passing
For parameter type `T`:
- argument passing follows move-by-default/copy rules (§6).

For parameter type `&T`:
- caller must pass an addressable location and use explicit `&` at the call site.
- no implicit borrowing in v1.

Mutating inputs requires `box<T>` parameters (including `box<array<T>>` for arrays).

### 10.4 Named arguments and defaults
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
There are two ways to obtain a `box<P>` from a concrete `box<T>`:

1) **Explicit cast** (always allowed when `T` implements `P`):
   - A value of type `box<T>` may be converted to `box<P>` with an explicit conversion, and only if `T` implements `P` structurally.

2) **Implicit coercion** (allowed only when `T` declares conformance `[P]`):
   - If `T` declares `P` in `struct[...]`, then a value of type `box<T>` is implicitly coercible to `box<P>` at coercion sites (e.g. `let` type annotation, argument passing, return).

Examples (cast syntax TBD):
```p7
struct[Printable] Vec2(
  x: float,
  y: float,
) {
  pub fn print(&self) -> unit { ... }
}

let v = box(Vec2(1, 2));

let p1: box<Printable> = v;                   // ok: implicit (Vec2 declares [Printable])
let p2: box<Printable> = v as box<Printable>; // ok: explicit cast (always allowed if Vec2 implements Printable)
```

Semantics:
- Converting `box<T>` to `box<P>` does not allocate a new `T`; it reinterprets the existing box handle with an associated dispatch table for `P`.
- If `T` does not implement `P`, the conversion is a compile-time error.

[[TODO]]: decide cast spelling:
- `v as Printable` (where `Printable` is a proto)
- or `v as box<Printable>`
- or `to_proto<Printable>(v)`

[[TODO]]: precisely define the set of coercion sites for implicit `box<T> -> box<P>`.

### 12.4 Dynamic dispatch
Calling a proto method on `box<P>` performs dynamic dispatch:
- `p.print()` invokes the concrete implementation for the dynamic type stored in `p`.

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

## 14. Error handling (`throw`, `try`) and typed throws

### 14.1 Throwing
`throw expr;` aborts evaluation and transfers control to the nearest enclosing `try`.

Constraints:
- Thrown values must be of an `enum` type (including payload enums if/when supported).
- `throw` is only permitted in functions declared with `fn[throws]` or `fn[throws<E>]`.

If a function is declared:
- `fn[throws] ...`: it may `throw` any enum value.
- `fn[throws<E>] ...`: it may `throw` only values of enum type `E`.

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

### 14.3 Calling `throws` functions (propagation vs handling)
Calling a function declared with `throws` is permitted only if:
1) The call is within the dynamic extent of a `try ... else ...` expression which handles any thrown value, or
2) The enclosing function is also declared with `throws` and the thrown type is compatible.

Compatibility rule (recommended for v1):
- `throws<E>` may be called only from `throws<E>` (exact match), unless handled by `try`.
- `throws` (unconstrained) may be called only from `throws` (unconstrained), unless handled by `try`.

---

## 15. Standard conversions and type checking

### 15.1 Numeric operations and coercions

#### 15.1.1 Integer overflow
For `int` arithmetic operations (`+`, `-`, `*`, and any other fixed-width integer arithmetic operators added in v1):
- If the mathematical result does not fit in signed 64-bit range, evaluation **traps** (runtime error).

A standard library (or prelude) function is provided for wraparound addition:
- `wrapping_add(a: int, b: int) -> int` computes `(a + b) mod 2^64`, interpreted as a signed two’s-complement `int`.

[[TODO]]: define additional wrapping/checked helpers:
- `wrapping_sub`, `wrapping_mul`
- `checked_add(a,b) -> ?int` (recommended; aligns with nullability)

#### 15.1.2 Numeric coercions
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

If the function is declared `fn[throws]` / `fn[throws<E>]`, hosts are encouraged to expose this as a structured result (e.g. `Ok(value)` / `Err(enum_value)`), but the concrete API is implementation-defined.

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
- Integer width is i64; float width is f64 (§3.1).
- Integer overflow traps by default; wraparound addition is available via `wrapping_add` (§15.1.1).
- Protos are structural and boxed-only (`box<P>`) (§12).
- `struct[...]` declares conformances and enables implicit behaviors:
  - `[Copy]` enables implicit copying for Copy-eligible structs (§6.3).
  - `[P]` enables implicit `box<T> -> box<P>` coercions for proto `P` (§12.3).
- Explicit operations/casts remain available based on structural properties:
  - `copy(x)` duplicates Copy-eligible values even without `[Copy]` (§6.4).
  - explicit `as box<P>` is available when `T` implements `P` (§12.3).
- Structs are tuple-like only for fields; blocks are only for methods (§11).
- `throw` is restricted to enum values and only permitted in `fn[throws]` functions (§14.1).
- `fn[...]` may include signature qualifiers `throws`, `throws<E>`, and (extension) `fiber` (§10.2, §20).

---

## 19. Open items / TODO list

1) Decide float NaN/Inf behavior details and conversions
2) Decide `string` default Copy policy (recommended: Copy by default)
3) Decide `copy(x)` surface syntax and naming
4) Decide `array<T>` default Copy policy
5) Define arrays: literal syntax, indexing, bounds behavior
6) Define boxed array mutation APIs and semantics
7) Define string: encoding, indexing, slicing
8) Define enum payload variants (if any)
9) Define error model: thrown value types, matching/patterns (now constrained to enums)
10) Define module system & visibility
11) Define host ABI/value representation and ownership transfer
12) Finalize proto cast syntax (`box<T>` -> `box<P>`) and runtime dispatch table caching
13) Define precise semantics of `*box` read/write and member auto-deref (including reads of non-`Copy` from a box)
14) Finalize shadowing type rule (whether explicit annotation may change type)
15) Finalize whether `&` can be taken of temporaries
16) Precisely define coercion sites for implicit `box<T> -> box<P>` when `T` declares `[P]`
18) Specify whether `try` can narrow thrown enum types in match-like else blocks
19) Fiber extension: specify borrow/view restrictions across `yield` (recommended: disallow `&T` live across `yield`)

---

## 20. Fiber extension (cooperative coroutines)

Status: Extension (optional in runtime / implementation).

Goal:
- Enable cooperative coroutines where script code can explicitly yield control back to the host (or a host-provided scheduler), preserving execution context and resuming later.

### 20.1 Enabling the extension
- When the Fiber extension is not enabled, `fn[fiber]` and `yield` are compile-time errors.
- When enabled, `fn[fiber]` and `yield` are available as specified below.

[[TODO]]: define how a program declares it requires the fiber extension (compiler flag, module import, or host configuration).

### 20.2 The `fiber` function qualifier
A function declared with `fn[fiber]` is a **fiber function**.

Properties:
- Fiber functions may suspend execution via `yield;`.
- Fiber functions are cooperatively scheduled: they run until they explicitly `yield`, `return`, or `throw`.

Calling convention constraints:
- `yield` is only valid inside a `fn[fiber]` function body.
- The runtime must provide a way for the host to create and resume fiber executions (§20.4).
- Direct calls to a fiber function require a fiber context (i.e. may only occur within another `fn[fiber]`), otherwise compile-time error.
- Non-fiber code should start fibers via a host/runtime API that returns a fiber handle (§20.4).

### 20.3 `yield;` statement
Form:
- `yield;`

Semantics:
- Suspends the current fiber execution.
- Transfers control to the entity that resumed the fiber (host or scheduler).
- When resumed again, execution continues immediately after the `yield;`.

Typing:
- `yield;` is a statement. It yields no value to p7 code (no resume inputs in v1).

Restrictions:
- `yield;` is only permitted inside a `fn[fiber]` function.

### 20.4 Host interop requirements for fibers
When the Fiber extension is enabled, the host/runtime must support:
- Creating a fiber execution from a `fn[fiber]` function and its arguments.
- Resuming a fiber execution.

A minimal host-driven protocol is:

- Create/start: produce a fiber handle `H` (opaque).
- Resume: `resume(H)` runs the fiber until it reaches one of:
  1) `yield;`   => reports `Yielded`
  2) returns    => reports `Returned(value)`
  3) throws     => reports `Threw(error_enum)`

Notes:
- Fibers are single-threaded and cooperative (no preemption).
- After `Returned` or `Threw`, the handle is complete and cannot be resumed further.

[[TODO]]: specify concrete API surface and mapping to host language.
[[TODO]]: specify whether yielding can be observed via debug hooks/profiling.

### 20.5 Interaction with `&T` borrowed views
Because `yield` suspends execution, values of type `&T` must not escape across suspension points.

In v1, to avoid introducing lifetime tracking, implementations must enforce a conservative restriction such as:

- A value of type `&T` must not be live across a `yield;` within a fiber function.

[[TODO]]: finalize and specify the exact static restriction enforced by the compiler (e.g. ban `&` entirely in `fn[fiber]` in v1, or ban only across yield).

---
End.
