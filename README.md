# nwscript-lsp

A language server for [NWScript](https://nwnlexicon.com/), the scripting language used in Neverwinter Nights: Enhanced Edition.

Built in Rust with [tower-lsp](https://github.com/ebkalderon/tower-lsp), featuring a hand-written recursive descent parser with Pratt expression parsing and error recovery for IDE-friendly partial ASTs.

## Features

- **Workspace indexing** -- scans all `.nss` files on startup, auto-discovers source dirs from `nasher.cfg`
- **Cross-file include resolution** -- follows `#include` graph with cycle detection, transitive symbol visibility
- **Diagnostics** -- real-time parse errors as you type, plus full compiler diagnostics on save (via bundled `nwn_script_comp`)
- **Document symbols** -- outline of functions, structs, constants, and global variables
- **Completion** -- all symbols across the workspace with auto-import for non-included files
- **Hover** -- type information and doc comments for functions, structs, and variables across files
- **Go to definition** -- jump to definitions across files, prefers implementations over forward declarations
- **Signature help** -- parameter hints when typing function calls (triggered on `(` and `,`)
- **Unused import detection** -- grayed-out `#include` directives with quick-fix removal
- **Code formatting** -- full document, range, and on-type formatting with configurable style (Allman/K&R braces, line width, include sorting, and more)

## Architecture

```
crates/
  parser/     # Zero-dependency NWScript parser library
              #   - Hand-written lexer (all tokens including trivia)
              #   - Full AST types (declarations, statements, expressions)
              #   - Recursive descent parser with Pratt expression parsing
              #   - Error recovery for partial ASTs from broken code
              #   - AST-based code formatter with comment preservation
  lsp/        # Language server binary
              #   - tower-lsp server with document management
              #   - Completion, hover, go-to-definition, document symbols
              #   - External compiler integration for diagnostics
              #   - Configurable directory exclusion and include paths
editors/
  vscode/     # VS Code extension (thin client)
bin/
  win64/      # Bundled nwn_script_comp compiler binary
```

## Building

### Prerequisites

- [Rust](https://rustup.rs/) (1.85+)
- [Node.js](https://nodejs.org/) (18+) for the VS Code extension

### Build the language server

```bash
cargo build --release -p nwscript-lsp
```

The binary will be at `target/release/nwscript-lsp.exe` (Windows) or `target/release/nwscript-lsp` (Linux/macOS).

### Package the VS Code extension

```bash
cd editors/vscode
npm install
npm run package          # esbuild bundle + minify
cp ../../target/release/nwscript-lsp.exe bin/
npx @vscode/vsce package --allow-missing-repository
```

This produces a `.vsix` file you can install in VS Code via **Extensions > Install from VSIX**.

## Installation

### VS Code

1. Build the language server: `cargo build --release -p nwscript-lsp`
2. Package the extension (see above)
3. Install the `.vsix` file in VS Code

For development, set `nwscriptLsp.serverPath` in VS Code settings to point to the built binary directly.

### Vanilla Scripts and Engine Built-ins

The LSP automatically finds your NWN:EE installation (via `NWN_ROOT` env var, Steam, Beamdog, or GOG paths) and extracts all vanilla `.nss` scripts from the game's KEY/BIF files. This gives you:

- Include resolution for vanilla scripts (`nw_i0_generic`, etc.) without copying them into your project
- Go-to-definition on vanilla functions opens the extracted source
- `nwscript.nss` engine built-in definitions are always available
- Workspace files always override vanilla scripts (same as the game engine)

If auto-detection fails, set the install path explicitly:

```json
"nwscriptLsp.nwnRoot": "C:/Program Files (x86)/Steam/steamapps/common/Neverwinter Nights"
```

### Compiler Diagnostics

For on-save diagnostics, the extension uses `nwn_script_comp`. It looks for a bundled copy next to the server binary, then falls back to `PATH`. For Nasher-based projects, run `nasher compile` at least once so the `.nasher/cache/` exists -- the LSP compiles against this cache to avoid false positives.

## Configuration

All settings are under the `nwscriptLsp` namespace in VS Code.

### General

| Setting | Default | Description |
|---------|---------|-------------|
| `compilerPath` | `""` | Path to `nwn_script_comp`. Empty = bundled or PATH. |
| `serverPath` | `""` | Path to `nwscript-lsp` binary. Empty = bundled or PATH. |
| `nwnRoot` | `""` | Path to NWN:EE installation. Empty = auto-detect from env/Steam/Beamdog/GOG. |
| `nwscriptNssPath` | `""` | Path to `nwscript.nss`. Empty = extracted from NWN install or searched in workspace. |
| `includeDirs` | `[]` | Additional source directories (added to those from `nasher.cfg`). |
| `excludeDirs` | `["node_modules", "target", "build", "output"]` | Directory names to skip when scanning. Dot-prefixed dirs always skipped. |

### Formatter

| Setting | Default | Description |
|---------|---------|-------------|
| `formatter.braceStyle` | `"nextLine"` | `"nextLine"` (Allman) or `"sameLine"` (K&R) |
| `formatter.maxLineWidth` | `120` | Line width before wrapping params one-per-line |
| `formatter.maxBlankLines` | `1` | Max consecutive blank lines |
| `formatter.sortIncludes` | `true` | Sort `#include` directives alphabetically |
| `formatter.trimTrailingWhitespace` | `true` | Remove trailing whitespace |
| `formatter.spaceAfterKeywords` | `true` | `if (x)` vs `if(x)` |
| `formatter.spaceInsideParens` | `false` | `( x )` vs `(x)` |
| `formatter.spaceAroundOperators` | `true` | `a + b` vs `a+b` |
| `formatter.spaceAfterComma` | `true` | `f(a, b)` vs `f(a,b)` |

Recommended VS Code settings for NWScript files:

```json
"[nwscript]": {
    "editor.formatOnSave": true,
    "editor.formatOnType": true,
    "editor.defaultFormatter": "krezk.nwscript-lsp"
}
```

## Project Structure Support

The LSP works with any NWScript project layout:

- **Nasher projects** -- reads `nasher.cfg` to discover source directories automatically
- **Multi-root workspaces** -- scans all workspace folders
- **Plain folders** -- indexes workspace root if no `nasher.cfg` is found
- **Custom layouts** -- use `includeDirs` and `excludeDirs` to fine-tune what gets indexed

## Roadmap

- [x] Cross-file symbol resolution (follow `#include` graph)
- [x] Workspace-wide indexing (scans all .nss files on startup)
- [x] Signature help (parameter hints on `(` and `,`)
- [x] Read `nasher.cfg` for include paths
- [x] Code formatting (Allman/K&R, include sorting, comment preservation)
- [x] Unused import detection with quick-fix removal
- [x] Auto-import on completion
- [x] Configurable directory exclusion
- [ ] Find references / rename
- [ ] Semantic tokens (syntax highlighting from AST)
- [ ] Local variable completion (variables within current function scope)

## License

MIT
