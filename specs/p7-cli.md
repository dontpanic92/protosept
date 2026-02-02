# p7-cli Specification

This document specifies how `p7` should parse command-line arguments to support both:

- **Direct script execution**: `p7 foo.p7 [args...]` (no `--` required)
- **Tooling subcommands**: `p7 run ...`, `p7 test ...` (use `--` to forward args)

The goal is a CLI that is ergonomic for scripting while remaining scalable and discoverable as a multi-command tool.

## Summary of User-Facing Behavior

### Direct script mode (Python-like)

- `p7 foo.p7 [script-args...]` runs `foo.p7`.
- Any tokens **after** the script path are passed to the script verbatim.
- No `--` is required to pass `-flags` to the script.

Examples:

- `p7 foo.p7 -n 10 --verbose`
  - Runs `foo.p7`
  - Script argv receives: `-n`, `10`, `--verbose`

- `p7 -v foo.p7 -n 10`
  - `-v` is a **p7 global option** (because it appears before the script)
  - Script argv receives: `-n`, `10`

- `p7 foo.p7 --help`
  - Runs `foo.p7`
  - Script argv receives: `--help` (this must not be intercepted by p7 help)

### Subcommand mode (tooling-like)

- `p7 run foo.p7 -- [script-args...]` runs `foo.p7`.
- In subcommands, `--` is used to disambiguate and forward args.

Examples:

- `p7 run foo.p7 -- -n 10`
  - Runs `foo.p7`
  - Script argv receives: `-n`, `10`

- `p7 run foo.p7 -n 10`
  - `-n` is interpreted as a **p7 run option** (and should error if unknown)

### REPL

- `p7` (with no script/subcommand) starts the REPL.
- `p7 repl` starts the REPL explicitly.

## Parsing Rules (Normative)

### Definitions

- **argv**: the full argument vector, including `argv[0]` (the program name)
- **token**: an element of argv other than `argv[0]`
- **flag token**: a token that starts with `-` (and is not the literal `-`)
- **`--` token**: literal `--`
- **script path token**: a token that ends with `.p7` (optionally also requiring it to exist on disk)
- **subcommand token**: one of the supported subcommand names (e.g. `run`, `test`, `check`, `build`, `fmt`, `repl`, `version`, `help`)

### Mode selection

`p7` MUST choose exactly one of the following modes:

1. **Subcommand mode**
2. **Direct script mode**
3. **REPL mode**

The selection algorithm is:

1) **Subcommand detection has highest priority**

- Scan tokens from left to right, stopping before any `--` token.
- Identify the **first non-flag token**.
- If that token matches a known subcommand name, select **subcommand mode**.

Rationale: subcommands should be predictable and discoverable (`p7 test`, `p7 run`, etc.).

2) If no subcommand is selected, attempt **direct script detection**

- Scan tokens from left to right.
- Treat `--` as the end of global option parsing during this scan.
- Select the first token that qualifies as a script path token (`*.p7`).
- If found, select **direct script mode**.

3) Otherwise select **REPL mode**

Rationale: with no explicit target, REPL is the most ergonomic default.

### Direct script mode argument splitting

When direct script mode is selected:

- Let `i` be the index of the script path token within argv.
- Let `global_argv = argv[0..i]` (program name plus any tokens before the script)
- Let `script_path = argv[i]`
- Let `script_args = argv[i+1..]` (all tokens after the script path)

`p7` MUST:

- Parse global options from `global_argv` only.
- Pass `script_args` verbatim to the program.

This guarantees that tokens like `--help` after the script path are not consumed by p7.

### Subcommand mode argument forwarding

When subcommand mode is selected:

- Parse argv normally as a subcommand invocation.
- For subcommands that run code (e.g. `run`), the script/program args MUST be collected as a trailing list, where `--` is required to separate them from subcommand flags.

This behavior is intentionally aligned with Go/Cargo conventions.

## Implementation Strategy (Recommended)

Use a **two-phase parser**:

1) **Manual dispatch scan** over raw argv to decide which mode to use.
2) Use `clap` (or similar) to parse:
   - Subcommand mode: parse full argv with subcommands
   - Direct script mode: parse only the global prefix (`global_argv`), then forward the rest

This avoids ambiguity and prevents `--help` from being intercepted when it appears after the script path.

## Test Matrix (Recommended)

Table-driven tests for `dispatch_from_argv` should include:

- `p7` → REPL
- `p7 repl` → subcommand repl
- `p7 foo.p7 --help` → direct script; script argv contains `--help`
- `p7 --help foo.p7` → p7 help (global)
- `p7 run foo.p7 -- -n 1` → subcommand run; args contain `-n`, `1`
- `p7 run foo.p7 -n 1` → error unless `-n` is a known `run` option
- `p7 -v foo.p7 -x` → direct script; global opts include `verbose`; script argv contains `-x`

## Non-Goals / Future Extensions

- Package selection rules (e.g. `p7 run .`) are not specified here.
- Module resolution flags and search paths are not specified here.
- The internal representation of "script argv" (including or excluding `script_path`) is implementation-defined; the examples assume `script_args` excludes the script path.
