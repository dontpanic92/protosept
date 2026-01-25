# p7 Language Specification (Draft)

Status: Draft  
Design goals (north star):
- **Statically typed scripting**: concise, readable, low ceremony.
- **Limited syntax/grammar**: features must pay rent in simplicity and interop.
- **Readability first**: prefer clarity and obvious semantics over brevity; avoid sigil-heavy syntax when it obscures ownership, aliasing, or lifetime-like constraints.
- **Correctness by default**: explicit nullability, explicit borrowing, explicit identity/mutation.
- **Host interop**: easy to embed; predictable runtime values and errors.

This document defines the *intended* semantics. Where implementation details are not finalized, sections use `[[TODO]]`.

---

## 0. Notation

- `T, U, V` are types.
- `x, y, z` are identifiers.
- `null` denotes the null value (only inhabits nullable types).
- "Slot" means a storage location introduced by `let`/parameters.

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
`fn`, `struct`, `enum`, `proto`, `let`, `pub`, `return`, `if`, `else`, `throw`, `throws`, `try`, `loop`, `break`, `continue`, `for`, `in`, `suspend`, `yield`, `ref`

[[TODO]]: confirm final keyword set; keep minimal.
Note: `suspend` and `yield` are reserved even though they are only valid when the Fiber extension is enabled (§21).

### 2.3 Comments
- Line comments: `// ...`
- Block comments: `/* ... */`

---

## 3. Types

### 3.1 Primitive types
p7 provides the following primitive types:

- `int`  
  Signed 64-bit two's-complement integer (i64).  
  **Overflow**: traps (unrecoverable panic; see §14.0) in v1 (§15.1).

- `float`  
  IEEE-754 binary64 floating-point (f64).  
  [[TODO]]: specify NaN/Inf behavior details and conversions.

- `bool`  
  Boolean. Values: `true`, `false`.  
  [[TODO]]: confirm literals exist as keywords or identifiers.

- `char`  
  A Unicode scalar value (i.e. a Unicode code point excluding surrogate range).  
  Intended for character-oriented scripting and iteration over `string`.  
  [[TODO]]: specify literal syntax (recommended: single quotes, e.g. `'a'`, `'\n'`, `'\u{1F600}'`).

- `unit`  
  The unit type, representing "no value". The only value is `()` [[TODO]] or implicit.

### 3.2 String type
- `string` is a built-in **immutable value type** representing textual data.
- The encoding is UTF-8.
- The semantic unit of iteration is `char` (Unicode scalar value).
- Copy/move semantics are defined in §6.
- `string` may internally share storage (e.g. copy-on-write), but this is not semantically observable.

Operations (v1 minimum):
- `len_chars(s: string) -> int` returns the number of `char` values in the string.  
  Note: this may be O(n) in the length of the string.
- `get_char(s: string, i: int) -> ?char` returns the `i`th character (0-based) or `null` if out of bounds.  
  Note: this may be O(n).
- `concat(a: string, b: string) -> string` returns concatenation. [[TODO]] exact spelling.

Indexing policy:
- p7 does not provide `s[i]` syntax for strings in v1.
  Rationale: character indexing is not naturally constant-time for UTF-8 strings; using an explicit function makes cost visible.

[[TODO]]: slicing semantics and APIs.

### 3.3 Array type
- `array<T>` is a built-in **immutable value type** representing a sequence of `T`.
- Value arrays cannot be mutated in place.
- In-place mutation and identity/aliasing are provided by `box<array<T>>` (§3.6, §7.4).

[[TODO]]: surface syntax for array indexing assignment is illegal for value arrays.
[[TODO]]: define boxed array mutation APIs (e.g., push/pop/set) and their signatures.

#### 3.3.1 Array literals (v1)
Array literals use square brackets:

- `[e1, e2, e3]` creates an `array<T>` where `T` is the element type.
- `[]` is permitted only when a contextual type is available (e.g. via annotation).

Type rule (v1):
- All elements in an array literal must have the same type `T` (after inference). No implicit numeric widening inside literals in v1. [[TODO]] numeric coercions.

Examples:
```p7
let xs = [1, 2, 3];              // array<int>
let ys: array<string> = [];      // ok
```

#### 3.3.2 Array indexing (v1)
Two ways to index arrays are provided:

1) Trap indexing:
- `a[i]` reads the element at index `i` (0-based).
- If `i` is out of bounds, evaluation traps (unrecoverable panic; see §14.0).

2) Checked indexing:
- `a.get(i)` returns `?T`, yielding `null` when out of bounds.

[[TODO]]: specify whether negative indices trap / return null (recommended: treat as out of bounds).

### 3.4 Nullable types
- `?T` is a nullable type: value is either `null` or a non-null `T`.

This aligns with a generic spelling `nullable<T>`:
- `?T` is syntactic sugar for `nullable<T>`.

Rules:
- `null` is only assignable to `?T`.
- `T` is not implicitly convertible to `?T` unless explicitly wrapped/promoted by a rule [[TODO]].
- Unwrapping rules are in §15.2.

Examples:
- `let x: ?int = null;`
- `let y: int = x;`  // error unless proven non-null via control flow

### 3.5 Borrow (reference view) type: `ref T`
p7 has **borrowed reference views**:

- `ref T` : a **read-only** borrowed view of an existing slot/sub-location holding a `T`.

Borrowed views are **non-escapable** (§7).

Views compose naturally with nullability:
- `ref ?T` is a view of a nullable slot/value of type `?T`.

> Important: `ref T` is *not* a heap box and is *not* an owned reference.

All shared mutation and escaping references are done via `box<T>` (§3.6, §7.4).

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
- `proto Name { ... }` defines a **conformance interface** for compile-time checking, and optionally a boxed dynamic-dispatch interface (§12).

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
- If a name is shadowed, the new binding must have the **same type** as the shadowed binding.

Rationale: keep shadowing predictable and avoid turning it into an untyped "variable reuse" mechanism.

### 5.3 Identity and mutation
Mutation in v1 is performed only through `box<T>`:
- field assignment on `box<Struct>` (e.g. `p.x = 1` where `p: box<Point>`)
- boxed cell update operations (e.g. `*b = value`) [[TODO]] exact syntax
- in-place mutation operations on boxed arrays (e.g. `a.push(x)` where `a: box<array<T>>`) [[TODO]] exact API

There is no direct in-place mutation of value structs:
- `s.x = 1` is illegal when `s: Struct` (non-box).

There is no direct in-place mutation of value arrays:
- in-place update of `a[i] = v` is illegal when `a: array<T>` (non-box).

---

## 6. Moves and copies (core rule set)

### 6.1 Move-by-default
For a value of type `T`:
- If `T` is not `Copy`, then:
  - `let b = a;` **moves** `a` into `b`.
  - After move, `a` is invalid to use (compile-time "moved" error).
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

A conformance name in `struct[...]` must be the name of a **proto**.

In p7, protos are divided into two categories:

1) **Constraint protos**:
   - Used only for compile-time conformance checking and enabling implicit behaviors.
   - They are *not* valid as runtime dynamic-dispatch types (they cannot appear as `box<P>`).

2) **Object protos**:
   - Used for compile-time conformance checking and enabling implicit behaviors.
   - Additionally, they may be used as runtime dynamic-dispatch types via `box<P>` (§12).

For each proto listed, the compiler:
1) Performs a compile-time conformance check (structural).
2) Enables certain *implicit* behaviors (coercions / operations) associated with that proto.

If a struct lists a proto that it does not satisfy, compilation fails.

Notes:
- `struct[...]` does **not** inject methods or fields into the type. It only checks and enables implicit behaviors described in this specification.
- The bracket list is about *implicitness*. A program may still use explicit casts/operations based on structural properties (see §12.3 and §6.4).

[[TODO]]: precise grammar for the bracket list, including whether duplicates are allowed (recommended: disallow) and whether order matters (recommended: no).

### 6.3 The `Copy` proto (constraint proto)
`Copy` is a built-in **constraint proto** that indicates that a type may be duplicated by duplicating its parts.

- A type `T` may satisfy (implement) `Copy` structurally, independent of whether it opts into implicit copying.
- A struct is treated as `Copy` for the purpose of *implicit* move/copy behavior only if it declares `Copy` in `struct[...]`.

In other words:
- Satisfying `Copy` is a structural property.
- Declaring `[Copy]` opts a struct into implicit copy behavior.

#### 6.3.1 Copy behavior
When a value of type `T` is copied:
- Primitives: duplicating a primitive duplicates the bits/value.
- Structs: copying duplicates each field (using each field's copy rule).
- Arrays: copying duplicates the array value and copies each element.
  - `array<T>` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are "as if" a value copy occurred.
- Strings: copying duplicates the string value.
  - `string` is immutable; implementations may optimize copying via shared storage (e.g. copy-on-write), but the semantics are "as if" a value copy occurred.
- Boxes: copying a `box<T>` copies the handle (aliases the same boxed cell). This is a shallow copy of the handle, not a deep copy of `T`.

#### 6.3.2 Structural conformance to `Copy` (Copy-eligibility)
A type `T` satisfies `Copy` (is Copy-eligible) iff it may be duplicated structurally.

- `int`, `float`, `bool`, `char`, `unit` satisfy `Copy` and is treated as `Copy` by default.
- `box<T>` satisfies `Copy` (handle copy) and is treated as `Copy` by default.
- `?T` satisfies `Copy` iff `T` satisfies `Copy`.
- `ref T` does not satisfy `Copy` (and views are non-escapable regardless; §7.3).
- `array<T>` satisfies `Copy` iff `T` satisfies `Copy`.
- `string` satisfies `Copy` and is treated as `Copy` by default (immutable value semantics; may share storage internally).

User-defined structs:
- A struct `S(...)` satisfies `Copy` iff all of its field types satisfy `Copy`.
- A struct is treated as `Copy` (i.e. copies implicitly) only if it declares `[Copy]`.

User-defined enums:
- An enum satisfies `Copy` iff all payload types (if any) satisfy `Copy`.
- Unit-only enums satisfy `Copy`.

### 6.4 Explicit copying
p7 provides an explicit copying operation:

- `copy(x)` : requires that the type of `x` satisfies `Copy`, and returns a copied value of the same type.

This operation may be used even if the type does not opt into implicit copy behavior via `struct[Copy]`.
Rationale: structural properties may be used explicitly, but implicit duplication must be opted into by declaring `[Copy]`.

### 6.5 The `Send` constraint proto

`Send` is a built-in **constraint proto** (see §12.2 for proto categories) that indicates a type represents a **deep-copyable pure value** with no identity or aliasing.

The `Send` constraint is primarily used by the Threading extension (§21) to control which types can be safely transferred across thread boundaries. However, `Send` is always available as a core language feature, independent of any extensions.

A type satisfies `Send` (is Send-eligible) if it is a pure value that can be deeply copied without aliasing concerns:

**Send-eligible types**:
- All primitive types (`int`, `float`, `bool`, `char`, `unit`) satisfy `Send`.
- `string` satisfies `Send` (strings are immutable values).
- `array<T>` satisfies `Send` iff `T` satisfies `Send` (arrays are immutable values).
- `enum` types satisfy `Send` iff all payload types (if any) satisfy `Send`.
- User-defined `struct` types may satisfy `Send` as specified in §6.5.1.

**Non-Send types**:

The following types do **not** satisfy `Send`:

- `box<T>`: Boxes have identity and support mutation (§3.6, §7.4). A `box<T>` represents a handle to shared, mutable state. Transferring a box between execution contexts would create aliasing (multiple handles to the same mutable state), which violates the isolation principle. Threading (§21) is the primary use case that requires this guarantee.
- `ref T`: Borrowed views are non-escapable (§7.3) and tied to the lifetime of the viewed slot on the stack. They cannot safely outlive their referent or be transferred to other execution contexts.
- Any user-defined type that transitively contains a field of type `box<T>` or `ref T`.

#### 6.5.1 Opt-in Send conformance for user-defined types

`Send` is **opt-in** for user-defined struct types:

- Struct authors must explicitly declare `Send` conformance using the conformance syntax:
  ```p7
  struct[Send] MyData(x: int, y: string) { }
  ```
- The compiler must verify that all fields satisfy `Send` before allowing the conformance.
- If any field does not satisfy `Send` (e.g., the struct transitively contains a `box<T>` or `ref T`), declaring `[Send]` is a compile-time error.

Rationale for opt-in:
- Explicit `[Send]` makes it visible in the struct declaration that the type is intended for use in isolated execution contexts.
- It prevents accidentally allowing types to cross context boundaries during prototyping.
- It provides a conservative starting point.

Note: The primary use case for `Send` is the Threading extension (§21), where Send-eligibility controls which types can cross thread boundaries.

[[TODO]]: Consider auto-derived Send in a future version: automatically derive `Send` for all structs whose fields satisfy `Send`, with an opt-out mechanism (e.g., `struct[!Send]`) for types that should not be Send even if fields are eligible. This would be particularly useful for the Threading extension.


---

## 7. Borrowed views (`ref T`) and boxes (`box<T>`)

### 7.1 Meaning of `ref T` (read-only view)
A borrowed view refers to an **existing storage location** (slot or sub-location).

If `r: ref T` refers to `x: T`:
- `*r` reads the current value of `x`.

### 7.2 Taking views
- `ref x` is allowed when `x` is addressable (slot or sub-location).

### 7.3 Non-escapable rule (hard rule in v1)
Values of type `ref T` **must not escape** their scope.

A view value cannot be:
- returned from a function
- assigned into a struct field
- assigned into an array element
- stored in any heap-allocated value (including `box<...>`)
- stored in globals/statics
- captured by closures (if/when closures exist)
- passed to host interop boundaries as a persistent value [[TODO]] (viewing may be supported only during a call)

Consequences:
- user-defined types cannot contain fields of type `ref ...`
- arrays cannot contain `ref ...` elements

This avoids needing escape analysis or lifetime tracking in v1.

### 7.4 Meaning of `box<T>`
A `box<T>` contains a `T` and provides:
- stable identity
- escapable storage
- shared mutation (mutation through a box is visible through all aliases)

Operations (surface syntax TBD):
- Construction: `box(expr)` allocates a new boxed cell containing `expr`.
- Write/set: `*b = expr` writes a new `T` into the cell [[TODO]].
- Read/deref:
  - If `T` is `Copy`, `*b` yields a value copy of type `T`.
  - If `T` is not `Copy`, reading the inner value by move is **not allowed** via `*b` in v1.
    Rationale: `box<T>` is an aliasable handle; allowing implicit move-out would require defining "moved-out" states or uniqueness.
- Replace (v1):
  - `replace(b, new_value)` writes `new_value` into the box and returns the previous value.
  - This permits moving non-`Copy` values out of a box without leaving it uninitialized.
- Member access auto-deref: `b.field` and `b.method(...)` access the inner value. [[TODO]] (recommended: yes).
- Field assignment: `b.field = expr` updates the inner struct field **in-place** (only valid when `b: box<S>`).
  - This is a direct interior update of the boxed cell's contents, not a desugaring to read-modify-write of `S`.

[[TODO]]: define the precise semantics of `*box` read/write and member auto-deref, including views of boxed contents (e.g. `ref *b`).

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

Note: `yield` is a statement/expression only under the Fiber extension (§21); it is not part of core expressions in v1.

### 8.2 Block expressions
A block `{ ... }` contains a sequence of statements.

Value of a block:
- If the final statement is an expression statement without a trailing semicolon, the block evaluates to that expression's value.
- Otherwise the block evaluates to `unit`.

### 8.3 `if` expression
`if condition then_expr else else_expr`

- `condition` must be `bool`.
- `then_expr` and `else_expr` must have compatible types.
- The `if` expression's type is the common type (or requires explicit conversions) [[TODO]].

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

[[TODO]]: define the exact "common type" rules (must be identical type in v1 recommended).

#### 8.5.5 Normal completion
A loop expression does not complete normally by reaching the end of its body; it runs until a `break` is executed, or until it throws.

#### 8.5.6 `break` and `step` interaction
- `break` does **not** execute the `step` clause.
- `continue` **does** execute the `step` clause.

#### 8.5.7 Interaction with `ref T` views
Because shadowing creates new bindings (new slots), a view `ref x` taken in one iteration refers to that iteration's binding and must not escape (§7). Views cannot be stored for use across iterations.

### 8.6 `try` expressions
See §14.

---

## 9. Statements

### 9.1 Statement forms
- `let` binding: `let x = expr;`
- expression statement: `expr;`
- `return expr;` or `return;` (returns `unit`)
- `throw expr;` (only valid in functions declared with `throws`; §14)
- `break;` and `break expr;` (only valid inside `loop` / `for`)
- `continue;` (only valid inside `loop` / `for`)
- `for` statement: `for x in expr { ... }` (§9.3)
- `yield;` (only valid in `suspend fn` when Fiber extension is enabled; §21)
- declarations (functions/types) [[TODO]] where allowed

### 9.2 Return semantics
Functions return the value of:
- an explicit `return`, or
- the last expression of the function body block (if not terminated by `;`), otherwise `unit`.

### 9.3 `for` statement (v1)
p7 provides a `for` statement for iteration over arrays and strings.

Form:
```p7
for x in expr { body }
```

Where:
- `expr` must have type `array<T>` or `string`.
- If `expr` is `array<T>`, then `x` has type `T`.
- If `expr` is `string`, then `x` has type `char`.

Semantics:
- `for` evaluates `expr` once.
- It then executes `body` once per element/character, in order.
- `break` and `continue` behave as in `loop`:
  - `break;` exits the loop.
  - `continue;` skips to the next iteration.

Binding behavior:
- `x` is a new binding each iteration (single-assignment, like all `let` bindings).
- Move/copy rules apply normally when binding `x` from the iterated value.

Desugaring (informative):
- `for` may be implemented equivalently to a `loop` over an internal index and repeated `get`/trap indexing.

Rationale:
- Adds essential scripting ergonomics without introducing a general iterator protocol in v1.

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

### 10.2 Function qualifiers and effects

Function declarations support two kinds of annotations that are **part of the function's type and calling contract**:

1. **Execution qualifiers** (before `fn`): modify how the function executes
2. **Effect qualifiers** (after return type): describe additional outcomes

#### 10.2.1 Execution qualifiers

Execution qualifiers appear as keywords **before** `fn`:

```p7
suspend fn name(params...) -> R { ... }
```

In v1, the only execution qualifier is:
- `suspend` — marks a suspendable function (cooperative coroutine); see §21.

#### 10.2.2 Effect qualifiers

Effect qualifiers appear **after** the return type:

```p7
fn name(params...) -> R throws { ... }
fn name(params...) -> R throws<E> { ... }
```

In v1, the effect qualifiers are:
- `throws` — function may throw any enum value
- `throws<E>` — function may throw only values of enum type `E`

When the return type is omitted (defaulting to `unit`), effect qualifiers follow the parameter list:

```p7
fn name(params...) throws { ... }
fn name(params...) throws<E> { ... }
```

#### 10.2.3 Combining qualifiers

Qualifiers may be combined:

```p7
suspend fn background_task() -> unit { ... }
fn risky_operation() -> int throws<MyError> { ... }
suspend fn async_fetch() -> Data throws<NetworkError> { ... }
```

Full grammar:
```
function_decl := [execution_qualifier] 'fn' name '(' params ')' ['->' type] [effect_qualifier] block

execution_qualifier := 'suspend'
effect_qualifier := 'throws' | 'throws' '<' enum_type '>'
```

### 10.3 Parameter passing
For parameter type `T`:
- argument passing follows move-by-default/copy rules (§6).

For parameter type `ref T`:
- caller must pass an addressable location and use explicit `ref` at the call site.
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
  pub fn length(self: ref Self) -> float {
    // ...
  }
}
```

Receivers in v1:
- `self` (by value; move/copy)
- `self: ref Self` (read-only view)

There is no `self: ref mut Self` in v1. In-place mutation APIs should use `box<Self>` parameters (or be expressed as free functions taking `box<T>`).

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

## 12. Protos (conformance interfaces and optional dynamic dispatch)

### 12.1 Overview
A `proto` defines a **structural conformance interface**: a set of requirements that a type may satisfy.

A concrete type `T` satisfies a proto `P` if `T` provides methods matching every required signature in `P`.

### 12.2 Proto categories: constraint protos vs object protos
Protos are divided into two categories:

1) **Constraint protos**:
   - Used only for compile-time checking and to enable implicit behaviors (via `struct[...]`).
   - They have no runtime dynamic-dispatch representation.
   - A constraint proto **must not** be used as a boxed proto type (`box<P>` is invalid).

   Example (built-in): `Copy` (§6.3).

2) **Object protos**:
   - Used for compile-time checking and to enable implicit behaviors (via `struct[...]`).
   - Additionally, they may be used as runtime dynamic-dispatch types via `box<P>`.

   Example: `Printable`.

- A user-declared `proto` is an object proto.
- Built-in protos may be either constraint protos (e.g. `Copy`) or object protos (none in v1).
- A constraint proto cannot be declared by users in v1 (only built-ins).

### 12.3 Proto declaration
Form:
```p7
proto Printable {
  fn print(self: ref Self) -> unit;
}
```

Rules:
- Method name must match exactly.
- Parameter types and return type must match exactly.
- Receiver must be `self: ref Self` in v1.

Restrictions in v1:
- Proto methods must use `self: ref Self` receiver only.
- Proto methods must not mention `Self` as a type (in parameters or return types). [[TODO]] may be added later.
- Overloads in proto are [[TODO]] (recommended: disallow in v1).

### 12.4 Proto values (object protos only)
Proto values are **boxed-only**:
- The only way to hold a dynamic-dispatch value of proto type `P` is via `box<P>`.
- There is no plain value of type `P`.

This applies only to **object protos**.
Constraint protos (e.g. `Copy`) cannot be used as `box<P>`.

Rationale:
- keeps dispatch and ownership uniform
- avoids hidden boxing
- makes sharing/escaping explicit

### 12.5 Converting a concrete box to a proto box (object protos only)
There are two ways to obtain a `box<P>` from a concrete `box<T>`:

1) **Explicit cast** (always allowed when `T` satisfies `P`):
   - A value of type `box<T>` may be converted to `box<P>` with an explicit conversion, and only if `T` satisfies `P` structurally.

2) **Implicit coercion** (allowed only when `T` declares conformance `[P]`):
   - If `T` lists `P` in `struct[...]`, then a value of type `box<T>` is implicitly coercible to `box<P>` at coercion sites (e.g. `let` type annotation, argument passing, return).

Examples (cast syntax TBD):
```p7
struct[Printable] Vec2(
  x: float,
  y: float,
) {
  pub fn print(self: ref Self) -> unit { ... }
}

let v = box(Vec2(1, 2));

let p1: box<Printable> = v;                   // ok: implicit (Vec2 declares [Printable])
let p2: box<Printable> = v as box<Printable>; // ok: explicit cast (always allowed if Vec2 satisfies Printable)
```

Semantics:
- Converting `box<T>` to `box<P>` does not allocate a new `T`; it reinterprets the existing box handle with an associated dispatch table for `P`.
- If `T` does not satisfy `P`, the conversion is a compile-time error.

[[TODO]]: decide cast spelling:
- `v as Printable` (where `Printable` is a proto)
- or `v as box<Printable>`
- or `to_proto<Printable>(v)`

[[TODO]]: precisely define the set of coercion sites for implicit `box<T> -> box<P>` when `T` declares `[P]`.

### 12.6 Dynamic dispatch (object protos only)
Calling a proto method on `box<P>` performs dynamic dispatch:
- `p.print()` invokes the concrete implementation for the dynamic type stored in `p`.

### 12.7 Downcasting / type tests
[[TODO]]: Provide runtime type tests and downcasts for proto boxes, e.g.:
- `p is Vec2`
- `p as Vec2` returning `?box<Vec2>` or throwing on failure

### 12.8 Nullability
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

### 14.0 Two failure channels: traps vs throws

p7 distinguishes between two kinds of runtime failures:

1. **Traps (panics)**: Unrecoverable runtime failures representing bugs or contract violations.
   - Examples: integer overflow, array out-of-bounds indexing (`a[i]`), force unwrap of `null` (`x!`).
   - A trap indicates a programming error: the preconditions of an operation were not met.
   - Traps **cannot be caught** by `try`. They propagate to the host/runtime as an unrecoverable error (panic).
   - The host/runtime may terminate execution, log an error, or take other appropriate action (e.g., triggering a debugger).
   
2. **Throws (typed errors)**: Recoverable errors represented by `enum` values.
   - Examples: parse failures, validation errors, domain-specific error conditions.
   - Thrown errors are **expected** and can be handled or propagated using `try` (§14.2, §14.3).
   - The type system tracks which functions may throw and what error types they throw (`throws` or `throws<E>`).

**Key distinction**:
- `try` expressions handle only **thrown** enum values. They **cannot catch traps**.
- If code traps during evaluation of a `try` expression, the trap propagates to the host (bypassing the `try`).

**Host-visible behavior** (v1):
When calling into p7 from the host, the host must be able to distinguish three outcomes:
- Normal return (function completed successfully with a value).
- Threw (function threw an enum value; this is recoverable and the host may handle it).
- Trapped/panicked (unrecoverable failure; host should treat as a fatal error or bug).

[[TODO]]: specify exact host API surface for observing these outcomes and their representation in the host runtime.

### 14.1 Throwing
`throw expr;` aborts evaluation and transfers control to the nearest enclosing `try`.

Constraints:
- Thrown values must be of an `enum` type (including payload enums if/when supported).
- `throw` is only permitted in functions declared with `throws` or `throws<E>`.

If a function is declared:
- `fn name() -> R throws { ... }`: it may `throw` any enum value.
- `fn name() -> R throws<E> { ... }`: it may `throw` only values of enum type `E`.

Examples:
```p7
fn parse(input: string) -> int throws<ParseError> {
  if input == "" {
    throw ParseError.EmptyInput;
  }
  // ...
}

fn do_something() throws {
  throw SomeError.Failed;
}
```

### 14.2 Try expressions
`try` is used both to **propagate** and to **handle** thrown enum values. Calls that may throw must be wrapped in a `try` form; there is no implicit propagation.

Forms:

1) **Propagation**:
- `try expr`

If `expr` throws, the thrown enum value is propagated out of the current function.

2) **Handling**:
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
- In the **handling** form, if `expr` throws, the `else` branch is evaluated and becomes the result of the `try`.
- In the **propagation** form, if `expr` throws, evaluation of the current function aborts and the thrown value is propagated to the caller.
- Pattern syntax and matching rules are [[TODO]].
- Type of a `try` expression is the common type of the normal result and the else result (handling form). In the propagation form, the type is the type of `expr`.

Examples:

Handling in a non-`throws` function:
```p7
fn b() -> int {
  let x = try a() else 0;
  x
}
```

Propagation from a `throws` function:
```p7
fn c() -> int throws {
  let x = try a(); // propagate if a() throws
  x
}
```

### 14.3 Calling `throws` functions (explicit propagation and handling)
Calling a function declared with `throws` or `throws<E>` requires an explicit `try` at the call site.

Rules:

1) **In non-`throws` functions**:
- A call to a `throws` / `throws<E>` function is permitted only in the **handling** form:
  - `try call_expr else ...`
- The propagation form `try call_expr` is a compile-time error in a non-`throws` function.

2) **In `throws` / `throws<E>` functions**:
- A call to a `throws` / `throws<E>` function must appear in one of the `try` forms:
  - `try call_expr` (propagate), or
  - `try call_expr else ...` (handle)

3) **Bare calls are not allowed**:
- Calling a `throws` function without `try` (e.g. `a()`) is a compile-time error, even inside a `throws` function.

Compatibility rule (recommended for v1):
- A function with `throws<E>` may propagate only from a function also declared `throws<E>` (exact match), unless handled locally.
- A function with `throws` (unconstrained) may propagate only from a function also declared `throws` (unconstrained), unless handled locally.
- Any thrown value may be handled locally using `try ... else ...` regardless of the enclosing function's `throws` annotation.

---

## 15. Standard conversions and type checking

### 15.1 Numeric operations and coercions

#### 15.1.1 Integer overflow
For `int` arithmetic operations (`+`, `-`, `*`, and any other fixed-width integer arithmetic operators added in v1):
- If the mathematical result does not fit in signed 64-bit range, evaluation **traps** (unrecoverable panic; see §14.0).

A standard library (or prelude) function is provided for wraparound and checked addition:
- `wrapping_add(a: int, b: int) -> int` computes `(a + b) mod 2^64`, interpreted as a signed two's-complement `int`.
- `checked_add(a: int, b: int) -> ?int`

#### 15.1.2 Numeric coercions
- allow implicit `int -> float` promotion in arithmetic/comparison
- require explicit conversion elsewhere

### 15.2 Nullability checks (v1 concrete rules)
Rules for using `?T`:

#### 15.2.1 Control-flow narrowing (v1)
If `x` has type `?T`, then in:

```p7
if x != null { ... } else { ... }
```

Inside the `then` branch, `x` is treated as type `T` (non-null).  
Inside the `else` branch, `x` is treated as `null`.

Narrowing applies only when `x` is a simple identifier (not an arbitrary expression) in v1.

#### 15.2.2 Null-coalescing (v1)
`x ?? default_expr`:
- If `x` is non-null, yields the inner `T`.
- Otherwise yields `default_expr`.

Type rule (v1):
- `default_expr` must have type `T`.

#### 15.2.3 Force unwrap (v1)
`x!`:
- Requires `x: ?T`.
- If `x` is non-null, yields the inner `T`.
- If `x` is `null`, evaluation traps (unrecoverable panic; see §14.0).

---

## 16. Memory model / runtime model (informative)

p7 uses a GC-based runtime. However, the language semantics are defined in terms of:
- value moves/copies
- borrowed views that alias slots (non-escapable)
- boxed identity containers (`box<T>`) that can escape and can be mutated

Implementation may represent values on stack or heap; this is not semantically observable.

[[TODO]]: specify runtime value set for host interop:
- int/float/bool/char/unit/null
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

If the function is declared with `throws` / `throws<E>`, hosts are encouraged to expose this as a structured result (e.g. `Ok(value)` / `Err(enum_value)`), but the concrete API is implementation-defined.

[[TODO]]: define API and error mapping.

### 17.2 Calling host functions from p7
Host may register functions callable by p7.

Requirements:
- Interop supports `?T` mapping to/from host null.
- Borrowed views (`ref T`) do not cross the boundary as persistent values.
  They may be passed to host only for the duration of a call, or disallowed entirely in v1 [[TODO]].
- Boxes (`box<T>`) are the primary mechanism for passing identity/mutable objects across the boundary.
- Proto boxes (`box<P>`) are the primary mechanism for passing dynamically-dispatched objects across the boundary.

### 17.3 Ownership rules
- Passing a value type `T` to host follows move/copy semantics.
- Boxes are handles; passing `box<T>` copies/moves the handle per rules in §6.

### 17.4 Generics and host interop

p7 generics (§19) are compile-time only and use monomorphization:
- **Exported entrypoints must be monomorphic**: Host-visible functions called from the host must have concrete types. Generic functions cannot be directly called from the host unless instantiated with specific type arguments at compile time.
- **No open generics at runtime**: There is no runtime representation of generic type parameters. All generics are resolved to concrete types during compilation.
- **Runtime polymorphism via `box<P>`**: For dynamic dispatch across the host boundary, use proto boxes (`box<P>`). Proto boxes provide runtime polymorphism without exposing generic type parameters to the host.

Example:
```p7
// Generic function (compile-time only; cannot be called directly from host)
fn identity<T>(x: T) -> T { return x; }

// Host-callable monomorphic function (ok)
fn identity_int(x: int) -> int { return identity(x); }

// Host-callable function using runtime polymorphism (ok)
fn process_printable(obj: box<Printable>) -> unit {
  obj.print();
}
```

Rationale:
- Keeps host interop simple and predictable.
- Avoids requiring hosts to understand p7's generic instantiation or type parameters.
- Proto boxes (`box<P>`) serve as the stable ABI boundary for polymorphic values.

---

## 18. Attributes (declaration metadata) (v1)

p7 supports **attributes**: typed metadata values attached to declarations. Attributes are intended for host interop (examining a compiled artifact) and future in-language reflection.

In v1, attributes are:
- **typed** (schema is defined by a `struct` declaration),
- **compile-time only** (they do not evaluate at runtime),
- **inert** (they do not affect typing or evaluation of the annotated program, except as specified by explicit future language/tooling features),
- **preserved** in the compiled artifact in a well-defined, host-visible form.

### 18.1 Where attributes may appear (v1)

An attribute list may appear immediately before a top-level declaration:

- `fn` declarations
- `struct` declarations
- `enum` declarations

Attributes apply to the next declaration item.

Example:
```p7
struct route(
  path: string,
  method: HttpMethod = HttpMethod.GET,
);

enum HttpMethod { GET, POST }

@route(path = "/users")
fn list_users() -> string { ... }
```

[[TODO]]: Whether attributes may be applied to `proto` declarations, struct fields, enum variants, function parameters, and local declarations.

### 18.2 Syntax

Attributes use `@` followed by an attribute constructor:

- `@AttrName(...)`

An attribute constructor has the same surface form as struct construction. The attribute name `AttrName` must resolve to a `struct` type name.

- Parentheses are required even for empty attributes: `@AttrName()`.

Multiple attributes are written by repeating attribute constructors:

```p7
@doc("Entrypoint")
@export(name = "main")
fn main() -> unit { ... }
```

### 18.3 Attribute values are typed struct literals (schema + defaults)

Each attribute is an instance of a `struct` type, constructed using the normal struct construction rules:

- required fields must be supplied
- optional fields may be omitted and defaulted using field default values
- named arguments must match declared field names
- argument types must match declared field types (no special coercions beyond normal rules)

Example (defaulted field):
```p7
struct route(
  path: string,
  method: string = "GET",
);

@route(path = "/users") // method defaults to "GET"
fn list_users() -> string { ... }
```

### 18.4 Attribute constant restrictions (v1)

Attribute constructors are restricted to **compile-time constant** arguments. In v1, the following types are permitted as attribute field types:

- primitive scalar types: `int`, `float`, `bool`, `char`, `unit`
- `string`
- enum types (including unit-only enums in v1)
- nullable types `?T` where `T` is a permitted attribute type
- array types `array<T>` where `T` is a permitted attribute type
- user-defined structs may appear as attribute field types (nested attribute objects) only if all nested fields recursively satisfy the permitted set above.

### 18.5 Ordering, duplicates, and identity

- Attributes are an **ordered list** attached to the declaration.
- Duplicate attributes are allowed and order is preserved.
  - Example: `@tag("a") @tag("b") fn f() { ... }` has two `tag` attributes in that order.
- Attributes have no runtime identity; they are pure metadata values.

### 18.6 Semantic effect (v1)

Attributes are **inert metadata**:
- They do not change typing, overload resolution, move/copy behavior, borrowing rules, or runtime semantics.
- A conforming implementation must still parse, type-check, and preserve attributes as specified in this section.

A future version of the language or toolchain may assign meaning to specific attributes (e.g. exporting entrypoints, documentation), but such behavior must be explicitly specified.

### 18.7 Host visibility and compiled representation (normative)

A conforming implementation that produces a compiled artifact (bytecode, native code, IR) must preserve attributes in a host-visible metadata form.

For each attributed declaration, the compiled artifact must expose:
- the declaration kind (`fn` / `struct` / `enum`)
- the declaration name (and module qualification when modules exist)
- the ordered list of attribute instances

### 18.8 Errors

It is a compile-time error if:
- an attribute name does not resolve to a `struct` type
- an attribute provides an unknown named field
- a required field is omitted
- a provided field value is not a compile-time constant
- any attribute field type is not permitted by §X.4

---

## 19. Generics

Status: v1 (compile-time).

### 19.1 Overview and design principles

p7 supports **compile-time generics** via monomorphization:
- Generic functions, structs, and enums are parameterized by type parameters.
- The compiler generates a distinct copy of the code for each concrete instantiation used in the program.
- There are **no open generic types at runtime**; all generics are resolved at compile time.

This design enables:
- Zero runtime overhead for generic abstractions
- Simple implementation without runtime type erasure or reified generics
- Straightforward interop with host languages (see §17.4)

### 19.2 Generic functions

Generic functions are declared with type parameters in angle brackets after the function name:

```p7
fn identity<T>(x: T) -> T {
  return x;
}

fn first<T>(arr: array<T>) -> ?T {
  if arr.len() > 0 {
    return arr[0];
  }
  return null;
}
```

- Type parameters are declared in angle brackets: `<T>`, `<T, U>`, etc.
- Type parameter names follow identifier rules (§2.1).
- Type parameters may be used in parameter types, return types, and local variable annotations.

Calling generic functions:
- Type arguments may be inferred from the arguments: `identity(42)` infers `T = int`.
- Type arguments may be explicitly provided [[TODO]]: `identity<int>(42)`.
- If inference is ambiguous or fails, explicit type arguments are required [[TODO]].

### 19.3 Generic structs

Structs may be parameterized by type parameters:

```p7
struct Pair<T, U>(
  first: T,
  second: U,
);

struct Vec<T>(
  elements: array<T>,
) {
  pub fn push(self: box<Self>, item: T) -> unit {
    // ... add to self.elements ...
  }
  
  pub fn get(self: ref Self, index: int) -> ?T {
    // ... return element at index ...
  }
}
```

Construction:
- Generic structs are constructed with explicit type arguments [[TODO]]: `Pair<int, string>(1, "hello")`.
- Type inference at construction sites is [[TODO]] (may be added later).

Methods on generic structs:
- Methods may use the struct's type parameters.
- Methods may introduce additional type parameters [[TODO]] (recommended: allowed).
- `Self` refers to the generic struct type with its parameters (e.g., `Vec<T>`).

### 19.4 Generic enums

Enums may be parameterized by type parameters:

```p7
enum Option<T> {
  Some(value: T),
  None,
}

enum Either<A, B> {
  Left(left: A),
  Right(right: B),
}
```

Note: p7 uses `throw`/`try` for error handling (§14), so `Result<T, E>` is not the primary error handling mechanism. `Option<T>` and `Either<A, B>` are shown as examples of neutral generic enums.

Usage:
```p7
let x: Option<int> = Option::Some(42);
let y: Option<int> = Option::None;

let z: Either<int, string> = Either::Left(1);
```

[[TODO]]: Enum payload variants are still being finalized (§13.1). The syntax shown above assumes payload support.

### 19.5 Type parameter bounds

Type parameters may be constrained by proto bounds using the syntax `T: P`:

```p7
fn print_boxed<T: Printable>(value: box<T>) -> unit {
  value.print();
}
```

Rules:
- A bound is specified as `T: P` where `P` is a proto.
- In v1, **only a single proto constraint** is allowed per type parameter.
- Multiple bounds (e.g., `T: P + Q`) are not supported in v1.
- There is **no `where` clause** in v1.

Semantics:
- A bound `T: P` means that any concrete type substituted for `T` must structurally satisfy proto `P`.
- The constraint is checked at each instantiation site.
- Inside the generic body, methods from `P` may be called on values of type `T` or `ref T`.

Constraint protos vs object protos:
- Bounds may use **constraint protos** (e.g., `Copy`, `Send`) or **object protos** (user-defined protos).
- Constraint protos (§12.2) are used only for compile-time checking and implicit behaviors; they do not support `box<P>`.
- Object protos (§12.2) support both compile-time checking and runtime dynamic dispatch via `box<P>`.

Example with `Copy`:
```p7
fn duplicate<T: Copy>(x: T) -> Pair<T, T> {
  return Pair(x, x); // ok: x is Copy, so it can be used twice
}
```

### 19.6 Instantiation and monomorphization

Monomorphization is the process of generating specialized code for each concrete instantiation of a generic:

- When a generic function/struct/enum is used with specific type arguments, the compiler generates a concrete version of the code with type parameters replaced by the actual types.
- Each distinct instantiation produces a separate copy of the code in the compiled output.
- Type parameters are resolved at compile time; there is no runtime representation of generic type variables.

Example:
```p7
fn identity<T>(x: T) -> T { return x; }

let a = identity(42);      // generates identity_int
let b = identity("hi");    // generates identity_string
```

The compiler generates two distinct functions: `identity_int` and `identity_string` (conceptually).

Implications:
- Code size grows with the number of distinct instantiations.
- No runtime type parameters or type erasure.
- Optimal performance: no runtime dispatch overhead for generic functions (unless using `box<P>` for dynamic dispatch).

### 19.7 Interaction with other features

#### 19.7.1 Generics and `box<T>`

Generic functions and types may use `box<T>` where `T` is a type parameter:

```p7
fn box_identity<T>(x: box<T>) -> box<T> {
  return x;
}

struct Container<T>(
  value: box<T>,
);
```

- `box<T>` is a boxed handle to a heap-allocated value of type `T`.
- When `T` is a type parameter, the box is monomorphized: each instantiation gets a distinct `box<ConcreteType>`.

#### 19.7.2 Generics and `ref T`

Generic functions may take borrowed views of generic types:

```p7
fn inspect<T>(x: ref T) -> unit {
  // ... read x ...
}
```

- `ref T` is a read-only view of a value of type `T`.
- The same borrowing rules (§7) apply.

#### 19.7.3 Generics and proto boxes (`box<P>`)

For runtime polymorphism, use proto boxes (`box<P>`) directly without generics:

```p7
// Accept box<P> directly without generics for runtime polymorphism:
fn call_print(obj: box<Printable>) -> unit {
  obj.print();
}
```

- `box<P>` provides runtime polymorphism via dynamic dispatch (§12.6).
- Unlike generic type parameters, `box<P>` is not monomorphized; it uses a single dispatch mechanism for all conforming types.

Generic functions may also use proto boxes when the proto type itself needs to vary:

```p7
// Generic over the concrete type T (with proto bound):
fn print_boxed<T: Printable>(value: box<T>) -> unit {
  value.print(); // ok: box<T> supports proto method calls when T satisfies Printable
}
```

However, in most cases where you need runtime polymorphism, accepting `box<P>` directly (without generics) is more straightforward.

### 19.8 Limitations in v1

The following generic features are **not included in v1**:

1. **No `where` clause**: Constraints are expressed only as bounds in the type parameter list (`T: P`).
2. **Single proto constraint per type parameter**: `T: P + Q` is not supported; use `T: P` only.
3. **No higher-kinded types**: Type parameters cannot themselves be generic (e.g., no `F<_>` or `F<G<T>>`).
4. **No generic protos**: Proto declarations cannot have type parameters in v1 [[TODO]].
5. **No associated types**: Protos cannot declare associated types in v1 [[TODO]].
6. **Limited type inference**: Type argument inference at generic function call sites is implementation-defined [[TODO]]; explicit type arguments may be required in some cases.

These features may be considered for future versions.

---

## 20. Open items / TODO list

1) Decide float NaN/Inf behavior details and conversions
2) Decide `string` default Copy policy (recommended: Copy by default)
3) Decide `copy(x)` surface syntax and naming
4) Decide `array<T>` default Copy policy (recommended: not Copy by default)
5) Define boxed array mutation APIs and semantics
6) Define string: escapes, concatenation spelling, slicing APIs
7) Define enum payload variants (if any)
8) Define error model: thrown value types, matching/patterns (now constrained to enums)
9) Define module system & visibility
10) Define host ABI/value representation and ownership transfer
11) Finalize proto cast syntax (`box<T>` -> `box<P>`) and runtime dispatch table caching
12) Define precise semantics of `*box` read/write and member auto-deref (including views of boxed contents)
13) Finalize shadowing type rule (whether explicit annotation may change type)
14) Finalize whether `ref` can be taken of temporaries
15) Precisely define coercion sites for implicit `box<T> -> box<P>` when `T` declares `[P]`
16) Specify whether `try` can narrow thrown enum types in match-like else blocks
17) Fiber extension: specify borrow/view restrictions across `yield` (recommended: disallow `ref T` live across `yield`)
18) Decide how a proto is classified as constraint vs object in the surface language (currently: only built-ins may be constraint protos) (§12.2)
19) Threading extension: Define message passing primitives (channels, send/receive APIs) and blocking/non-blocking semantics (§22.9)
20) Threading extension: Define exact host API surface for thread management (wait, join, cancel) (§22.8)
21) Threading extension: Decide whether `spawn_thread` should return a thread handle value to p7 code (§22.4)
22) Generics: Decide explicit type argument syntax for generic function calls (e.g., `identity<int>(42)`) (§19.2)
23) Generics: Decide type inference rules at generic struct construction sites (e.g., `Pair(1, "hi")` vs `Pair<int, string>(1, "hi")`) (§19.3)
24) Generics: Decide whether methods on generic structs may introduce additional type parameters (§19.3)
25) Generics: Finalize enum payload variant syntax and integration with generics (§19.4)
26) Generics: Consider generic protos (protos with type parameters) for future versions (§19.8)
27) Generics: Consider associated types in protos for future versions (§19.8)

---

## 21. Fiber extension (cooperative coroutines)

Status: Extension (optional in runtime / implementation).

Goal:
- Enable cooperative coroutines where script code can explicitly yield control back to the host (or a host-provided scheduler), preserving execution context and resuming later.

### 21.1 Enabling the extension
- When the Fiber extension is not enabled, `suspend fn` and `yield` are compile-time errors.
- When enabled, `suspend fn` and `yield` are available as specified below.

[[TODO]]: define how a program declares it requires the fiber extension (compiler flag, module import, or host configuration).

### 21.2 The `suspend` execution qualifier
A function declared with `suspend fn` is a **suspendable function**.

Form:
```p7
suspend fn name(params...) -> R { ... }
suspend fn name(params...) -> R throws<E> { ... }
```

Properties:
- Fiber functions may suspend execution via `yield;`.
- Fiber functions are cooperatively scheduled: they run until they explicitly `yield`, `return`, or `throw`.
- Fibers are single-threaded and non-preemptive at the language level: the runtime does not interrupt execution in the middle of a statement/expression.

**Borrowed view restrictions in fiber functions (v1)**:
- Fiber functions must not use `ref T` types in parameters or local variables.
- The `ref x` view-taking expression is disallowed in fiber function bodies.
- Rationale: Borrowed views are stack-bound and non-escapable. Suspending execution via `yield` would allow views to outlive their referents across suspension points, violating safety guarantees.

Calling convention constraints:
- `yield` is only valid inside a `suspend fn` function body.
- A fiber function may be:
  - started from p7 via `spawn` (§21.5), or
  - started from the host via an embedding API (§21.4).

Direct calling constraints (recommended for v1):
- A `suspend fn` function may be called directly only from within another `suspend fn` function.
- Attempting to call a `suspend fn` function from a non-suspend function is a compile-time error.
Rationale:
- `yield` requires a fiber resumption context.

### 21.3 `yield;` statement
Form:
- `yield;`

Semantics:
- Suspends the current fiber execution.
- Transfers control to the entity that resumed the fiber (host or scheduler).
- When resumed again, execution continues immediately after the `yield;`.

Typing:
- `yield;` is a statement. It yields no value to p7 code (no resume inputs in v1).

Restrictions:
- `yield;` is only permitted inside a `suspend fn` function.

### 21.4 Host interop requirements for fibers
When the Fiber extension is enabled, the host/runtime must support:
- Creating a fiber execution from a `suspend fn` function and its arguments (start/spawn).
- Resuming a fiber execution.

A minimal host-driven protocol is:

- Create/start: produce a fiber handle `H` (opaque).
- Resume: `resume(H)` runs the fiber until it reaches one of:
  1) `yield;`   => reports `Yielded`
  2) returns    => reports `Returned(value)`
  3) throws     => reports `Threw(error_enum)`

Notes:
- After `Returned` or `Threw`, the handle is complete and cannot be resumed further.
- A host may ignore a fiber (never resume it). If ignored, it remains suspended indefinitely unless cancelled or dropped (see §21.7).

[[TODO]]: specify concrete host API surface and mapping to host language.

### 21.5 Spawning fibers from p7 (`spawn`)
In addition to host-started fibers, p7 code may create fibers using `spawn`.

Form:
```p7
spawn f(arg1, arg2, ...);
```

Where:
- `f` must refer to a function declared with `suspend fn`.
- `spawn` is a statement (returns `unit`) in v1.

Semantics:
- `spawn` requests creation of a new fiber execution of `f(arg1, arg2, ...)`.
- The newly created fiber is initially **runnable** and begins execution when (and only when) it is first resumed by a scheduler/host policy.
- The act of spawning does not itself run the new fiber.

Host visibility and control:
- Every successful `spawn` triggers the host hook `on_fiber_spawn` (§21.6) with a handle for the new fiber.
- The host may choose to:
  - schedule and resume the fiber,
  - defer it,
  - ignore it (never resume it),
  - cancel it (if cancellation exists; [[TODO]]).

Rationale:
- Allows scripts to start background behaviors while keeping the host in full control of scheduling and resource budgets.

[[TODO]]: decide whether `spawn` should return a `FiberHandle` value to p7 code (recommended later; keep statement-only in v1 to minimize surface area).

### 21.6 Host hook: observing fibers spawned by p7 (`on_fiber_spawn`)
To preserve embedding control, runtimes that enable the Fiber extension must provide a host hook that is invoked whenever p7 code spawns a new fiber:

- Hook name (conceptual): `on_fiber_spawn(handle: FiberHandle, info: FiberSpawnInfo) -> unit`

Where:
- `FiberHandle` is an opaque host value that can be passed to `resume`.
- `FiberSpawnInfo` is implementation-defined metadata. Recommended fields:
  - spawned function name (if available)
  - optional source location (if available)
  - optional parent fiber handle (if spawned from within a fiber) [[TODO]]

Semantics:
- The runtime invokes the hook synchronously during `spawn` execution (i.e. before `spawn` completes).
- The host may record the handle and decide scheduling policy externally.

Constraints:
- The hook must not itself resume the new fiber re-entrantly while `spawn` is still executing, unless the implementation explicitly guarantees re-entrancy safety.

### 21.7 Scheduling policy (informative)
Scheduling is not part of the core language semantics. A fiber yields control only at explicit `yield`, `return`, or `throw` boundaries; when and whether it is resumed is controlled by the host/runtime policy.

Example host policy for games:
- Host resumes selected fibers at most once per frame (or on a fixed tick).
- `yield;` represents a cooperative checkpoint; a frame-based policy can treat each yield as "pause until next frame".

### 21.8 Interaction with `ref T` borrowed views
Because `yield` suspends execution, values of type `ref T` must not escape across suspension points.

In v1, to avoid introducing lifetime tracking, implementations must enforce a conservative restriction such as:

- A value of type `ref T` must not be live across a `yield;` within a fiber function.

As specified in §21.2, the recommended restriction for v1 is to disallow `ref` usage entirely in fiber functions (no parameters/locals of type `ref T`, no `ref x` view-taking). This matches common restrictions in coroutine-based systems.

---

## 22. Threading extension

Goal:
- Enable p7 code to request thread spawning for concurrent execution while keeping the host/runtime in full control of OS thread management, scheduling, and resource budgets.
- Provide actor-like isolation where threads do not share mutable state, preventing data races and simplifying reasoning about concurrent execution.

### 22.1 Enabling the extension

- When the Threading extension is not enabled, `spawn_thread` is a compile-time error.
- When enabled, `spawn_thread` is available as specified below.
- The `Send` constraint proto (§6.5) is always available, regardless of whether the Threading extension is enabled.

[[TODO]]: define how a program declares it requires the Threading extension (compiler flag, module import, or host configuration).

### 22.2 Send-gated transfer

The Threading extension uses the `Send` constraint proto (defined in §6.5) to enforce compile-time safety for cross-thread value transfer.

**Threading-specific Send requirements**:
- All arguments passed to `spawn_thread` must have types that satisfy `Send` (§22.4).
- The return type of functions used with `spawn_thread` must satisfy `Send` (or return `unit`) (§22.5).
- Throwable enum types used in threaded functions must satisfy `Send` (§22.5).

These requirements ensure that only deep-copyable pure values can cross thread boundaries, preventing shared mutable state and aliasing across threads.

### 22.3 Threading model: actor-like isolation

**Isolation guarantees**:
- **No shared memory**: Threads do not share mutable state. Each thread has its own isolated memory space.
- **Send-gated transfer**: Only values whose type satisfies `Send` may be transferred between threads (as function arguments or eventual message passing). Attempting to pass a non-`Send` value across thread boundaries is a compile-time error.
- **No aliasing across threads**: Because `box<T>` and `ref T` are not `Send`, there is no way for two threads to hold references to the same mutable object or stack slot.

This model prevents data races by construction and simplifies reasoning about concurrent execution.

### 22.4 Spawning threads from p7 (`spawn_thread`)

p7 code may request creation of a new thread execution using `spawn_thread`.

Form:
```p7
spawn_thread f(arg1, arg2, ...);
```

Where:
- `f` must refer to a function (need not be `suspend fn`; regular functions are allowed).
- All arguments `arg1, arg2, ...` must have types that satisfy `Send`. If any argument's type does not satisfy `Send`, it is a compile-time error.
- `spawn_thread` is a statement (returns `unit`).

Semantics:
- `spawn_thread` requests creation of a new thread execution of `f(arg1, arg2, ...)`.
- The newly created thread is initially **runnable** and begins execution when (and only when) the host/runtime schedules it.
- The act of spawning does not itself start the new thread execution; the host controls scheduling.
- The calling thread continues execution immediately after `spawn_thread` (non-blocking).

Host visibility and control:
- Every successful `spawn_thread` triggers the host hook `on_thread_spawn` (§22.7) with a handle for the new thread.
- The host may choose to:
  - schedule the thread on an OS thread or thread pool,
  - defer its execution,
  - ignore it (never run it),
  - limit the number of concurrent threads based on resource budgets.

Rationale:
- Allows scripts to request concurrency while keeping the host in full control of thread creation, scheduling, and resource limits.
- The host can implement various threading strategies (OS threads, green threads, thread pools, etc.) without changing the p7 language semantics.

[[TODO]]: decide whether `spawn_thread` should return a thread handle value to p7 code (recommended for future versions to enable waiting/joining; keep statement-only initially to minimize surface area).

### 22.5 Thread completion outcomes

When a thread completes execution (reaches the end of its function), the outcome is one of:

1. **Returned(value)**: The function returned a value. The function used with `spawn_thread` must have a return type that satisfies `Send` (or returns `unit`), otherwise it is a compile-time error. If the return type satisfies `Send`, the host can observe and retrieve the value across the thread boundary.

2. **Threw(error_enum)**: The function threw an error (§14.1). The thrown enum type must satisfy `Send`. If the enum does not satisfy `Send`, it is a compile-time error. All throwable enums used in threaded functions must be `Send`, enabling the host to observe the error details across the thread boundary.

3. **Trapped(panic)**: The function trapped (§14.0). Traps are unrecoverable panics. When a thread traps:
   - The trap terminates the entire thread, including all fibers scheduled on that thread (if fibers are enabled; see §22.6).
   - Other threads in the program are **not** affected. Traps do not propagate across thread boundaries.
   - The host is notified of the trap outcome (host-specific mechanism; see §22.8).

Contrast with single-threaded execution:
- In single-threaded (non-extension) p7, a trap terminates the entire program.
- In the Threading extension, a trap terminates only the current thread, enabling supervision patterns where a parent thread can monitor worker threads and take corrective action upon trap (e.g., restart the worker, log the error, shut down gracefully).

### 22.6 Interaction with fibers (§21)

When both the Fiber extension (§21) and the Threading extension are enabled:

**Fiber pinning**:
- **Fibers are pinned to a single thread**: A fiber does not migrate between threads. Once created, a fiber remains on the thread where it was spawned.
- Each thread may have multiple fibers scheduled on it (cooperative multitasking within the thread).
- Fibers on different threads are isolated and cannot directly share memory.

**Send rules for fiber arguments**:
- Fibers are always spawned on the same thread where `spawn` is called (fibers are pinned to their creation thread).
- Because fiber spawning is always thread-local, arguments to `spawn` do not need to satisfy `Send`.
- This is in contrast to `spawn_thread`, where arguments must satisfy `Send` because the thread may execute on a different OS thread.

Note: If a future version allows fibers to be spawned on a different thread (cross-thread fiber creation), `Send` constraints would apply to those arguments. This is not part of v1.

**Trap boundaries**:
- If a fiber traps (§14.0), the trap terminates the entire thread, including all other fibers on that thread.
- Fibers cannot trap in isolation; the trap propagates to the containing thread.

### 22.7 Host hook: observing threads spawned by p7 (`on_thread_spawn`)

To preserve embedding control, runtimes that enable the Threading extension must provide a host hook that is invoked whenever p7 code spawns a new thread:

- Hook name (conceptual): `on_thread_spawn(handle: ThreadHandle, info: ThreadSpawnInfo) -> unit`

Where:
- `ThreadHandle` is an opaque host value that represents the thread. The host can use this handle to schedule, wait for, or cancel the thread.
- `ThreadSpawnInfo` is implementation-defined metadata. Recommended fields:
  - spawned function name (if available)
  - optional source location (if available)
  - optional parent thread handle (if spawned from within a thread) [[TODO: specify purpose - debugging/tracing or lifecycle management]]

Semantics:
- The runtime invokes the hook synchronously during `spawn_thread` execution (i.e., before `spawn_thread` completes).
- The host may record the handle and decide scheduling policy externally.

Constraints:
- The hook must not itself start execution of the new thread re-entrantly while `spawn_thread` is still executing, unless the implementation explicitly guarantees re-entrancy safety.

### 22.8 Thread completion observability (host)

The host/runtime must be able to observe when a thread completes and its outcome:

- The host may provide an API (host-specific, not part of p7 language) to wait for or poll thread completion.
- When a thread completes, the host can observe:
  - `Returned(value)`: if the return type satisfies `Send`, the host can retrieve the value.
  - `Threw(error_enum)`: if the enum satisfies `Send`, the host can retrieve the error details.
  - `Trapped`: the host is notified of the trap (panic). No value is returned.

Example use case (informative, host-side pseudo-code):
```
// Host code (not p7 syntax)
// After p7 script executes: spawn_thread worker_fn(42);
let handle = /* obtained from on_thread_spawn hook */;
match wait_for_thread(handle) {
  ThreadResult.Returned(val) => { /* use val */ },
  ThreadResult.Threw(err) => { /* handle error */ },
  ThreadResult.Trapped => { /* log panic, restart worker, etc. */ },
}
```

[[TODO]]: Define exact host API surface for thread management (wait, join, cancel). This is recommended for a future version; for now, the extension defines the language-side semantics only.

### 22.9 Message passing and channels (future)

The current Threading extension specifies thread spawning and argument passing. Future versions may add:

- **Channels** or **message queues** for sending messages between running threads.
- **Receive primitives** to read messages from a channel.
- All message types sent through channels must satisfy `Send`.

[[TODO]]: Define message passing primitives, channel APIs, and blocking/non-blocking receive semantics. This is deferred to a future version of the Threading extension.

---
End.
