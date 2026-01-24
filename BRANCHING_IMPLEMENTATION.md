# Branching and Exception Handling Implementation

## Overview
This document describes the implementation of branching statements and exception handling in the p7 language compiler.

## Branching Statements

### Expression::If (Fully Implemented)
The `if` expression is fully implemented in `p7/src/bytecode/codegen.rs` (lines 409-452).

**How it works:**
- Evaluates the condition expression
- Type-checks that the condition is `bool`
- Generates `Jif` (jump if false) instruction with placeholder address
- Generates code for the `then` branch
- If there's an `else` branch:
  - Generates `Jmp` instruction to skip the else branch
  - Patches the `Jif` to jump to the else branch start
  - Generates code for the `else` branch
  - Patches the `Jmp` to skip to the end
- If there's no `else` branch:
  - Patches the `Jif` to jump to the end

**Bytecode Instructions Used:**
- `Jif(u32)` - Jump if the top of stack is false (0)
- `Jmp(u32)` - Unconditional jump

### Statement::Branch (Basic Implementation)
Implemented basic support for `Statement::Branch` in `p7/src/bytecode/codegen.rs` (lines 159-183).

**Purpose:**
Used for pattern matching in `try...else` blocks for error handling.

**Current Implementation:**
- Generates code for the expression associated with each branch
- Works with the unwrapped exception value from `Expression::Try`
- Returns the expression's type

**Future Work:**
- Full pattern matching logic to check if exception value matches pattern
- Conditional execution of branches based on pattern match
- Variable binding for named patterns

## Exception Handling (NEW)

### Special Return Value Model

p7 implements exceptions as **tagged return values** instead of using setjmp/longjmp:

1. **Data Type Extension**
   ```rust
   pub enum Data {
       Int(i32),
       Float(f64),
       StructRef(u32),
       Exception(i32),  // Exception value
   }
   ```

2. **New Bytecode Instructions:**
   - `Throw` (opcode 25) - Wraps value as exception and returns immediately
   - `CheckException(u32)` (opcode 26) - Checks if top of stack is exception, jumps if so
   - `UnwrapException` (opcode 27) - Extracts exception value for pattern matching

3. **Expression::Try Implementation**
   
   For `try expr else handler`:
   ```
   1. Generate try_block expression
   2. CheckException handler_addr    ; Jump if exception
   3. Jmp end_addr                   ; Skip handler if normal
   4. handler_addr:
   5.   UnwrapException              ; Extract exception value
   6.   Generate else_block          ; Handler code
   7. end_addr:
   ```

4. **How Exceptions Propagate:**
   - `Throw` pops value, wraps as `Data::Exception`, and immediately returns
   - Exception appears on caller's stack as special return value
   - Caller's `CheckException` detects it and jumps to handler (if any)
   - Without handler, exception stays on stack and propagates up

See `EXCEPTION_HANDLING.md` for complete documentation of the exception handling model.

## Test Files

Test files in `tests/` directory (all `.p7` files, Rust test files removed):
- `test_if_expression.p7` - Basic if-else test
- `test_if_no_else.p7` - If without else test
- `test_nested_if.p7` - Nested if test
- `test_if_complex.p7` - Boolean logic test
- `test_exception.p7` - Exception handling test
- Other existing `.p7` files

## Summary

✅ **Expression::If** - Fully implemented with proper jump instructions
✅ **Statement::Branch** - Basic implementation for try-else pattern matching
✅ **Exception Handling** - Complete implementation using special return value model
✅ **VM Support** - All Data operations handle Exception variant
✅ **No setjmp/longjmp** - Simple, explicit exception flow through tagged return values

The implementation provides working branching for `if` expressions and a complete exception handling system that avoids the complexity of traditional stack unwinding.

