# Protosept Programming Language

Protosept is a statically-typed, scripting-oriented language.
This repository contains a Rust implementation of a compiler + bytecode interpreter, a minimal CLI, and a VS Code extension for basic language support.

## Language goals and features

- Language spec (draft): `specs/protosept-language.md`
- CLI argument behavior spec: `specs/p7-cli.md`


Protosept’s design centers on making code easy to review and reason about, especially in AI coding era:

- **Statically typed, scripting feel**: lightweight syntax and fast iteration, backed by compile-time type checking.
- **Auditability-first**: syntax and semantics aim to make intent, data flow, and “what happens at runtime” obvious during human review.
- **Explicit data semantics**: signatures distinguish plain values (`T`), borrowed views (`ref<T>`), and owned GC-heap handles (`box<T>`).
- **Correctness by default**: nullability is explicit (e.g. `?T`), and the language favors making error-prone behavior visible at the type level.
- **Tooling-friendly**: the compiler can support shorthands for authoring, while tooling can canonize code to a more explicit, unambiguous form for review.
- **Embedding/host interop**: the ownership/borrowing model is meant to map cleanly onto host systems for predictable integration.

### Feature tour (snippet)

The syntax below is intended to showcase the “shape” of the language (and largely matches what’s exercised in `tests/`).

```p7
// Values vs borrows vs heap handles
fn add_one(x: ref<int>) -> int {
  *x + 1
}

// Protos (structural interfaces) + dynamic dispatch via box<Proto>
proto Printable {
  fn print(self: ref<Printable>) -> int;
}

fn use_printable(p: box<Printable>) -> int {
  p.print()
}

// Different concrete types can be used where a `Printable` is expected.
// Conformance is checked structurally (required methods + signatures).
struct Dog(
  pub name: string,
) {
  pub fn print(self: ref<Dog>) -> int {
    42
  }
}

// Explicit proto conformance declaration. Compiler will ensure Cat implements Printable.
struct[Printable] Cat(
  pub name: string,
) {
  pub fn print(self: ref<Cat>) -> int {
    99
  }
}

// Enums + pattern matching
enum ErrorType(
  ErrorA,
  ErrorB,
  ErrorC
);

fn[throws] thrower(code: int) -> int {
  if code == 1 { throw ErrorType.ErrorA; }
  if code == 2 { throw ErrorType.ErrorB; }
  if code == 3 { throw ErrorType.ErrorC; }
  7
}

fn main() -> int {
  // Explicit nullability + coalescing
  let maybe: ?int = null;
  let base = maybe ?? 10;

  // Borrowing (ref) and deref. `&base` is a short hand of `ref(base)`
  let bumped = add_one(&base);

  // Heap allocation (box) + cast to protocol for dynamic dispatch
  // Sigil ^Dog is a short hand of `box(Dog)`
  let dog = ^Dog(name = "Rex");
  let cat = ^Cat(name = "Mittens");

  // box<Cat> can be implicitly upcast to box<Printable> since the conformance is explictly declared
  let printed = use_printable(dog as ^Printable) + use_printable(cat);

  // try/else with pattern matching on thrown values
  let recovered = try thrower(3) else {
    _: ErrorType.ErrorA => 1,
    _: ErrorType.ErrorB => 2,
    _: _ => 999,
  };

  bumped + printed + recovered
}
```

## Workspace layout

- `p7/` — core library crate: lexer/parser/semantic analysis, bytecode, interpreter.
- `p7-cli/` — CLI binary crate. Builds the `p7` executable.
- `unp7/` — helper crate used for disassembly/debugging.
- `std/` — standard library modules (loaded by the CLI for `import std.*`).
- `tests/` — `.p7` test files run by the CLI test harness.
- `p7-vscode/` — VS Code extension (syntax highlighting + language config).

## Build

Requires a recent Rust toolchain (this workspace uses `edition = "2024"`).

```bash
cargo build
```

To build an optimized binary:

```bash
cargo build --release
```

## CLI (p7)

The CLI binary is `p7` (from the `p7-cli` crate).

Run via Cargo:

```bash
cargo run -p p7-cli -- --help
```

### Run a script

Direct script mode (Python-like):

```bash
cargo run -p p7-cli -- path/to/script.p7
```

Notes:

- The CLI currently compiles and runs the script using entrypoint function name `main`.
  Your script should define `fn main() { ... }`.
- Tokens after the script path are forwarded as “script args” per the CLI spec, but the runtime does not currently expose argv to the program.

### Subcommands

- `run`: explicit run mode; requires `--` to forward script args

```bash
cargo run -p p7-cli -- run path/to/script.p7 -- --any -args you-want
```

- `repl`: starts a minimal REPL shell (evaluation is not implemented yet)

```bash
cargo run -p p7-cli -- repl
```

- `version`:

```bash
cargo run -p p7-cli -- version
```

### Standard library resolution

When the CLI resolves imports:

- `import std.*` loads from the repository’s `std/` directory (found relative to the `p7` executable).
- Other module paths are resolved relative to the directory of the entry script.
- `import builtin` is always available (bundled into the `p7` crate).

## Tests

The CLI has a small test harness (`p7 test`) that scans `.p7` files for functions tagged with `@test(...)` and executes them.

Run all tests in `tests/`:

```bash
cargo run -p p7-cli -- test
```

Run a single test file:

```bash
cargo run -p p7-cli -- test tests/test_basic_operations.p7
```

Compile-fail tests: add a line starting with `// compile_fail` anywhere in the `.p7` file.

## VS Code extension

The VS Code extension lives in `p7-vscode/` and currently provides:

- `.p7` language registration
- basic bracket/comment configuration
- a TextMate grammar for syntax highlighting

To try it locally, open `p7-vscode/` in VS Code and use the extension development workflow (Run → Start Debugging).

## Status / known limitations

This is an early-stage implementation.
A few notable current limitations (non-exhaustive):

- REPL does not evaluate code yet.
- Script argument forwarding exists at the CLI level, but argv is not surfaced inside the runtime.
- The CLI does not currently print the return value of `main`.

For intended semantics and design goals, see `specs/protosept-language.md`.
