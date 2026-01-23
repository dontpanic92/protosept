# p7 Language Specification (Draft)

Status: Draft  
Design goals (north star):
- **Statically typed scripting**: concise, readable, low ceremony.
- **Limited syntax/grammar**: features must pay rent in simplicity and interop.
- **Correctness by default**: explicit nullability, explicit mutability, explicit borrowing.
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

[[TODO]]: module/import system and visibility rules at module boundaries.

Top-level executable statements are not allowed in v1; execution begins by calling an entrypoint function via host embedding (e.g., `run_p7_code(contents, "main")`).

---

## 2. Lexical structure

### 2.1 Identifiers
Identifiers start with `_` or a letter and continue with letters, digits, or `_`.

### 2.2 Keywords (reserved)
`fn`, `struct`, `enum`, `let`, `mut`, `pub`, `return`, `if`, `else`, `throw`, `try`

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
- `string` is a built-in value type representing textual data.
- Copy/move semantics are defined in §6.

[[TODO]]: encoding (UTF-8 recommended), indexing semantics (bytes vs codepoints).

### 3.3 Array type
- `array<T>` is a built-in value type representing a sequence of `T`.

[[TODO]]: surface syntax for array literals, indexing, length, iteration.

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

### 3.5 Reference (borrow) types
p7 has **borrowed reference** types:

- `&T` : read-only borrowed reference to an existing slot/sub-location holding a `T`.
- `&mut T` : mutable borrowed reference to an existing mutable slot/sub-location holding a `T`.

Borrowed references are **non-escapable** (§7).

References compose naturally with nullability:
- `&?T` is a reference to a nullable slot/value of type `?T`.

> Important: `&T` / `&mut T` are *not* heap boxes and are *not* owned references.

### 3.6 User-defined types
- `struct Name(...) { ... }` defines a nominal product type with fields (tuple-like) and an optional method block.
- `enum Name { ... }` defines a nominal sum type.

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
Double-quoted strings: "hello"  
[[TODO]]: escapes.

### 4.4 Boolean literals
`true`, `false` [[TODO]].

### 4.5 Null literal
`null` literal exists and has type `?T` only when context supplies `T`.  
Examples:
- `let x: ?int = null;`
- `return null;` only valid when function returns `?T`.

---

## 5. Bindings, storage, and mutability

### 5.1 Slots and `let`
A binding introduces a slot:

- `let x = expr;` introduces an **immutable slot**.
- `let mut x = expr;` introduces a **mutable slot**.

Slot mutability is a static property used to validate assignments and borrows.

### 5.2 Assignment to slots
- Assignment `x = expr` is valid only if `x` is a mutable slot.
- Otherwise it is a compile-time error.

### 5.3 Mutability and aliasing intent
p7’s mutability model is **slot-based**:
- You may only mutate through a mutable slot or a mutable borrow of a mutable slot.
- There is no promise of deep immutability of heap graphs; the promise is about writes through the language’s mutation operations.

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
- Copying a string duplicates its storage and copies its bytes/code units.

Eligibility:
- A struct may be `Copy` only if all fields are `Copy` and the struct explicitly opts in.
- `array<T>` is `Copy` iff `T` is `Copy` [[TODO]] (recommended: yes).
- `string` is `Copy` iff it opts in; recommended: allow but note it implies allocation.

[[TODO]]: Syntax for opting into Copy: e.g. `struct A [Copy](...) { ... }` or similar.

### 6.3 Explicit copying and cloning
p7 may provide explicit operations:
- `copy(x)` : requires `T: Copy`, returns a copied value
- `clone(x)` : requires `T: Clone`, returns a duplicated value

[[TODO]]: Whether `Clone` exists in v1; recommended: postpone until needed.

### 6.4 Drop / destruction
[[TODO]]: whether p7 exposes deterministic destructors. Likely **no** in v1 (GC-based runtime).

---

## 7. Borrowed references: `&T` and `&mut T`

### 7.1 Meaning
A borrowed reference refers to an **existing storage location** (slot or sub-location).

If `r` refers to `x`:
- `*r` reads the current value of `x`
- `*r = v` updates `x` only when `r: &mut T`

Example (intended behavior):
```p7
let mut x = 1;
let r = &mut x;
x = 2;
*r    // evaluates to 2
```

### 7.2 Taking borrows
- `&x` is allowed when `x` is addressable (slot or sub-location).
- `&mut x` is allowed only when `x` is a mutable slot/sub-location.

Illegal:
- `let x = 1; let r = &mut x;`  // cannot mut-borrow immutable slot

### 7.3 Non-escapable rule (hard rule in v1)
Values of type `&T` and `&mut T` **must not escape** their scope.

A borrow value cannot be:
- returned from a function
- assigned into a struct field
- assigned into an array element
- stored in any heap-allocated value
- stored in globals/statics
- captured by closures (if/when closures exist)
- passed to host interop boundaries as a persistent value [[TODO]] (borrowing may be supported only during a call)

Consequences:
- user-defined types cannot contain fields of type `&...` or `&mut ...`
- arrays cannot contain `&...` or `&mut ...` elements

This avoids needing escape analysis or lifetime tracking in v1.

### 7.4 Borrow exclusivity rules
[[TODO]]: Choose one of:
- (A) No aliasing rules enforced (easiest, but can surprise)
- (B) Simple borrow rules: at most one `&mut` at a time, no `&` while `&mut` exists, etc. (Rust-like but can be simplified)

Recommended for simplicity & correctness: **a simplified Rust-like rule** enforced lexically within a function body:
- You cannot take `&mut x` if there exists an active borrow of `x` (either `&` or `&mut`) whose scope overlaps.
- You cannot take `&x` if there exists an active `&mut x` whose scope overlaps.

[[TODO]]: define “active borrow scope” precisely (likely lexical: until end of statement/block).

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
- `try` expressions (error handling)

### 8.2 Block expressions
A block `{ ... }` contains a sequence of statements.

Value of a block:
- If the final statement is an expression statement without a trailing semicolon, the block evaluates to that expression’s value.
- Otherwise the block evaluates to `unit`.

Examples:
```p7
let x = {
  let a = 1;
  a + 2   // no semicolon => value is 3
};

let y = {
  let a = 1;
  a + 2;  // semicolon => value is unit
};
```

### 8.3 `if` expression
`if condition then_expr else else_expr`

- `condition` must be `bool`.
- `then_expr` and `else_expr` must have compatible types.
- The `if` expression’s type is the common type (or requires explicit conversions) [[TODO]].

### 8.4 Operators and precedence
[[TODO]]: specify full operator set and precedence table.

---

## 9. Statements

### 9.1 Statement forms
- `let` binding: `let x = expr;`, `let mut x = expr;`
- expression statement: `expr;`
- `return expr;` or `return;` (returns `unit`)
- `throw expr;`
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

For parameter type `&T` / `&mut T`:
- caller must pass an addressable location and use explicit `&` / `&mut` at the call site.
- no implicit borrowing in v1.

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
  pub fn add(self, other: Self) -> Self {
    Self(self.x + other.x, self.y + other.y) [[TODO]] // placeholder if tuple constructor differs
  }
}
```

[[TODO]]: `Self(...)` constructor syntax for tuple-like structs.

### 11.3 Construction
Struct values are constructed by calling the struct name:
- `Point(1, 2)`
- with named args: `Point(y = 2, x = 1)`

[[TODO]]: whether a `Self(...)` expression exists inside methods.

### 11.4 Field access and assignment
- Access: `p.x`
- Assignment: `p.x = v` only allowed if `p` is mutable (slot or `&mut` borrow), per §5.3.

### 11.5 Methods (static dispatch sugar)
Receiver forms:
- `self` : by-value receiver (move/copy)
- `&self` : read-only borrow receiver
- `&mut self` : mutable borrow receiver

Method call desugaring:
- `a.add(b)` desugars to `Vec2.add(a, b)` (or a mangled free function) with static resolution.

No inheritance or dynamic dispatch.

---

## 12. Enums

### 12.1 Declaration
```p7
enum SomeErrors {
  NumberIsNot42,
  [[TODO]]: payload variants? e.g. Number(i:int)
}
```
v1 may support only unit variants (names only). Payload variants are [[TODO]].

### 12.2 Values and namespacing
Enum variants are referenced as:
- `SomeErrors.NumberIsNot42`

[[TODO]]: whether variants are in scope unqualified.

---

## 13. Error handling (`throw`, `try`)

### 13.1 Throwing
`throw expr;` aborts evaluation and transfers control to the nearest enclosing `try`.

[[TODO]]: whether thrown values must be of an enum type or any value is allowed.

### 13.2 Try expressions
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

## 14. Standard conversions and type checking

### 14.1 Numeric coercions
[[TODO]]: decide numeric coercions.
Recommendation for scripting:
- allow implicit `int -> float` promotion in arithmetic/comparison
- require explicit conversion elsewhere

### 14.2 Nullability checks
Rules for using `?T`:
- You cannot use a `?T` where `T` is required unless:
  - you check for non-null in control flow and the compiler narrows the type, or
  - you use an explicit unwrap operator `!` [[TODO]].
- Provide `??` operator: `x ?? default` [[TODO]].

---

## 15. Memory model / runtime model (informative)

p7 uses a GC-based runtime. However, the language semantics are defined in terms of:
- value moves/copies
- borrowed references that alias slots (non-escapable)

Implementation may represent values on stack or heap; this is not semantically observable.

[[TODO]]: specify runtime value set for host interop:
- int/float/bool/unit/null
- string
- array
- struct instances
- enum values
- (no borrowed references as persistent values)

---

## 16. Host interop (v1 requirements)

### 16.1 Calling into p7
Host calls a named p7 function with arguments and receives either:
- a returned value, or
- an error/exception object if `throw` escapes.

[[TODO]]: define API and error mapping.

### 16.2 Calling host functions from p7
Host may register functions callable by p7.

Requirements:
- Interop supports `?T` mapping to/from host null.
- Borrowed references (`&T` / `&mut T`) do not cross the boundary as persistent values.
  They may be passed to host only for the duration of a call, or disallowed entirely in v1 [[TODO]].

### 16.3 Ownership rules
- Passing a value type `T` to host follows move/copy semantics.
- Arrays/strings are values; host receives either owned copies or shared views depending on ABI [[TODO]].

---

## 17. Summary of chosen decisions (from discussion)

- Move-by-default (§6.1).
- Both `&T` and `&mut T` exist (§7).
- All `&...` are non-escapable (hard rule in v1) (§7.3).
- Nullability uses `?T` prefix (sugar for `nullable<T>`) (§3.4).
- Structs are tuple-like only for fields; blocks are only for methods (§11).

---

## 18. Open items / TODO list

1) Finalize integer/float widths
2) Decide if `string` is `Copy` by default (copy allocates)
3) Decide `copy(x)`/`clone(x)` existence and naming
4) Define borrow exclusivity precisely (simple lexical rule recommended)
5) Define arrays: literal syntax, indexing, bounds behavior
6) Define string: encoding, indexing, slicing
7) Define enum payload variants (if any)
8) Define error model: thrown value types, matching/patterns
9) Define module system & visibility
10) Define host ABI/value representation and ownership transfer
11) Decide on implicit int->float promotion rules
12) Define `Self(...)` construction syntax for tuple-like structs inside method blocks

---
End.