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
lcl = { index = "https://example.invalid/releases/v1/lcl.index.toml" }

[native]
extensions = []
```

`kind` is either `library` or `executable` and defaults to `executable` for
backward compatibility. `source` defaults to `src`. The package root module
defaults to `src/mod.p7` for libraries and `src/main.p7` for executables.
`entry` overrides that default and must remain inside the source directory.

Package and module path segments use Protosept identifier syntax. Dependency
keys must match the dependency package's declared name.

Each dependency selects exactly one source:

- `path` is a package directory relative to the declaring package.
- `index` is one exact artifact-index URL. The declaration contains no
  checksum or version range; integrity pins are written to `p7.lock`.

Artifact-index URLs must use HTTPS. `file://` URLs have identical resolution
semantics and are supported for local release testing. HTTP, Git dependencies,
registry names, version ranges, and source entries containing both `path` and
`index` are rejected.

A path package may depend on path or artifact packages. A downloaded artifact
package may depend on other artifact indexes, but may not contain path
dependencies. This prevents a cached package from escaping its extraction
root and keeps the downloaded graph reproducible.

## Artifact index and archives

An artifact index is versioned TOML:

```toml
version = 1

[package]
name = "utility"
version = "1.2.3"

[dependencies]
support = { index = "https://example.invalid/releases/v1/support.index.toml" }

[targets.aarch64-apple-darwin]
url = "utility-1.2.3-aarch64-apple-darwin.tar.gz"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "tar.gz"

[targets.x86_64-apple-darwin]
url = "utility-1.2.3-x86_64-apple-darwin.tar.gz"
sha256 = "..."
format = "tar.gz"

[targets.x86_64-unknown-linux-gnu]
url = "utility-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
sha256 = "..."
format = "tar.gz"

[targets.x86_64-pc-windows-msvc]
url = "utility-1.2.3-x86_64-pc-windows-msvc.tar.gz"
sha256 = "..."
format = "tar.gz"
```

`version = 1` and `format = "tar.gz"` are the only initially supported
values. Archive URLs may be absolute HTTPS or `file://` URLs, or relative URLs
resolved with standard URL semantics against the index URL. Target names are
the four canonical triples shown above. A host outside those triples, or an
index without the current host target, produces an error listing the requested
and available targets.

The archive is a complete package root: its root contains exactly one regular
`p7.toml`, the source tree, and any package-relative native extensions or
notices. Its manifest package name, version, and complete dependency source
map must match the index. Only regular files and directories are extracted.
Absolute paths, parent traversal, backslash paths, duplicate paths, symbolic
links, hard links, and special files are rejected.

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

Project commands write lockfile format version 2. A representative mixed graph
is encoded as:

```toml
version = 2

[[package]]
name = "app"
version = "0.1.0"
checksum = "<path package source checksum>"

[package.source]
kind = "path"
path = "."

[package.dependencies.utility]
index = "https://example.invalid/releases/v1/utility.index.toml"

[[package]]
name = "utility"
version = "1.2.3"

[package.source]
kind = "artifact"
index = "https://example.invalid/releases/v1/utility.index.toml"
index_sha256 = "<sha256 of the exact downloaded index bytes>"

[package.source.targets.aarch64-apple-darwin]
url = "https://example.invalid/releases/v1/utility-1.2.3-aarch64-apple-darwin.tar.gz"
sha256 = "<archive sha256>"
format = "tar.gz"

# The other index targets are recorded in the same form.

[package.dependencies.support]
index = "https://example.invalid/releases/v1/support.index.toml"
```

Path packages retain root-relative source identities and checksums of their
manifest and `.p7` source files. Artifact packages record the exact index URL,
the SHA-256 of the observed index bytes, and every target archive's resolved
absolute URL, SHA-256, and format. Recording all targets makes one committed
lockfile portable across supported hosts. Direct dependency source maps are
recorded so an extracted manifest can be checked against the locked graph.

For an artifact dependency with no matching lock entry, resolution trusts the
contents returned by the exact HTTPS index URL, verifies the selected archive
against the index checksum, and writes all pins above. Once a matching lock
entry exists, it is authoritative: normal commands do not refetch or silently
accept a changed index. They select the current target from the lock, fetch its
locked archive URL if needed, verify the locked checksum, and verify the
archive metadata against the lock. Changing an index URL or deliberately
refreshing its contents requires lockfile regeneration.

Version 1 path-only lockfiles remain readable and are upgraded when written.
Version 1 cannot represent artifact packages; a version 1 lockfile encountered
with an artifact dependency produces an explicit regeneration error. No
registry compatibility, semantic-version solving, signatures, authentication,
offline switch, or cache-management command is implied by version 2.

## Artifact cache

Indexes, archives, and extracted packages are stored below the operating
system's per-user cache directory in Protosept's `artifacts-v1` cache.
Index objects are keyed by the SHA-256 of their bytes; archive and extracted
package objects are keyed by the locked archive SHA-256. Downloads and
extractions are created beside their destination and atomically renamed only
after verification succeeds. Existing archive objects are rehashed before
use, and malformed cache markers or package manifests are reported as
corrupted cache entries rather than trusted.

Resolution errors distinguish invalid or unavailable URLs, network and file
I/O failures, unsupported targets or archive formats, checksum mismatches,
unsafe archives, corrupt cache entries, and package/index/lock metadata
mismatches.

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
