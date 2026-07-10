# Protosept Package and Project Format

Status: initial implementation

## Manifest

A project is rooted by `p7.toml`.

```toml
[package]
name = "example"
version = "0.1.0"
kind = "executable"
source = "src"
entry = "src/main.p7"

[dependencies]
utility = { path = "../utility" }

[native]
extensions = []
```

`kind` is either `library` or `executable` and defaults to `executable` for
backward compatibility. `source` defaults to `src`. The package root module
defaults to `src/mod.p7` for libraries and `src/main.p7` for executables.
`entry` overrides that default and must remain inside the source directory.

Package and module path segments use Protosept identifier syntax. Dependency
keys must match the dependency package's declared name.

The initial implementation supports path dependencies. Git dependencies and a
package registry remain future work.

## Module identities

Modules have canonical package-qualified names. Given package `example`:

```text
src/main.p7         -> example.main
src/ui/window.p7    -> example.ui.window
src/net/mod.p7      -> example.net
```

Imports resolve as follows:

- `import utility.text;` is an absolute import from a declared dependency.
- `import .helper;` resolves from the importing module's parent.
- `import _.shared.log;` resolves from the importing package root.

A package may import its own modules and modules from direct dependencies.
Transitive dependencies are not directly visible unless they are also declared.

## Lockfile

Project commands write `p7.lock`. It records:

- Lockfile format version.
- Package names and versions.
- Path source identities relative to the root project.
- SHA-256 checksums of each package manifest and `.p7` source files.
- Direct dependency names.

Path dependency checksums detect local source changes; path sources remain
local and are not treated as immutable registry artifacts. Relative identities
such as `path+.` and `path+../utility` keep lockfiles portable.

## Commands

```text
p7 check [project-dir]
p7 build [project-dir]
p7 run [project-dir]
p7 test [project-dir] [test-file]
```

`check` compiles the package graph. `build` writes
`target/<name>-<version>.p7bc`. Both commands support library and executable
packages. `run` invokes `main` for executable roots and rejects library roots.
`test` discovers `.p7` files under the root package's `tests` directory and
compiles them with access to the package graph.

Direct script execution remains available and behaves as an implicit legacy
package.

## Native extensions

Native extension artifacts are paths relative to their package root:

```toml
[native]
extensions = ["native/libexample.dylib"]
```

Paths must remain inside the package. At execution time dependency extensions
load before the root package's extensions, and every extension registers
through the versioned C ABI before script module initialization. `check` and
`build` validate artifact paths but do not load executable native code.
