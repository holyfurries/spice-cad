# Local Luau modules

Choose File → Examples → Reusable typed modules (also in the command palette)
to install the example project in your user data directory without overwriting
existing files. Open other project entry files with File → Open, or run:

```sh
cargo run -- --model models/modular/main.luau
cargo run -- --check-model models/modular/main.luau
cargo run -- --export-model models/modular/main.luau output/modular-print
cargo run -- --export-game models/modular/main.luau output/modular-game
```

The directory containing the entry file is the project root. No manifest is needed.
Use literal relative imports, such as `local bracket = require("./bracket")`.
Files use the `.luau` extension, which may be omitted from imports. Nested imports
resolve from the importing file. Imports may use `./../` within the project root.
Absolute paths, packages, dynamic imports, cycles, and symlinks escaping the root
are rejected. Projects are limited to 64 files and 4 MiB of combined source.

A module returns a value, usually a typed factory function or a table of functions.
Use `cad` directly inside modules; the same sandbox and geometry limits apply to
all modules together. Each module evaluates once per model run, so repeated imports
share its returned value. Put parameter controls and `cad.show` calls in the entry
file to keep properties and source navigation useful.

The existing Luau language server resolves imports for typed completion, hover,
and definitions. Running a model also checks imported code and its callers before
kernel work. Function annotations and exported type aliases carry across modules.
For example, a module can declare `export type Options = { width: number }`, and
its caller can use `local options: bracket.Options = { width = 30 }`.

Changes to imported files invalidate the preview and analysis caches. Automatic
preview rebuilds after the usual debounce; with automatic preview disabled, use
Run. Source changes on disk preserve the existing unsaved-buffer conflict flow.
Exports capture the source graph as `model.luau` and `module-N.luau` files, rewriting
only import paths. Keep these files together to rebuild without the original
project directory. Live editor diagnostics retain original file positions; build
diagnostics name the staged module when an imported file has an error.
