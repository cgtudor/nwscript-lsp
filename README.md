# nwscript-lsp

A language server for [NWScript](https://nwnlexicon.com/), the scripting language used in Neverwinter Nights: Enhanced Edition.

Built in Rust with [tower-lsp](https://github.com/ebkalderon/tower-lsp), featuring a hand-written recursive descent parser with Pratt expression parsing and error recovery for IDE-friendly partial ASTs.

## Features

- **Diagnostics** — real-time parse errors as you type, plus full compiler diagnostics on save (via bundled `nwn_script_comp`)
- **Document symbols** — outline of functions, structs, constants, and global variables
- **Completion** — functions, structs, constants, keywords, with snippet support for function parameters
- **Hover** — type information and doc comments for functions, structs, and variables
- **Go to definition** — jump to function, struct, and variable definitions

## Architecture

```
crates/
  parser/     # Zero-dependency NWScript parser library
              #   - Hand-written lexer (all tokens including trivia)
              #   - Full AST types (declarations, statements, expressions)
              #   - Recursive descent parser with Pratt expression parsing
              #   - Error recovery for partial ASTs from broken code
  lsp/        # Language server binary
              #   - tower-lsp server with document management
              #   - Completion, hover, go-to-definition, document symbols
              #   - External compiler integration for diagnostics
editors/
  vscode/     # VS Code extension (thin client)
bin/
  win64/      # Bundled nwn_script_comp compiler binary
```

### Design principles

1. **Real parser, not TextMate hacks** — the parser produces a proper AST with source spans, not token-position guesswork
2. **Error recovery** — the parser skips to synchronization points on errors, producing partial ASTs from incomplete code
3. **Trivia-aware** — the lexer preserves comments and whitespace for hover documentation extraction
4. **Compiler for truth** — diagnostics come from `nwn_script_comp` (the real NWScript compiler), not a reimplementation

## Building

### Prerequisites

- [Rust](https://rustup.rs/) (1.85+)
- [Node.js](https://nodejs.org/) (18+) for the VS Code extension

### Build the language server

```bash
cargo build --release -p nwscript-lsp
```

The binary will be at `target/release/nwscript-lsp.exe` (Windows) or `target/release/nwscript-lsp` (Linux/macOS).

### Build the VS Code extension

```bash
cd editors/vscode
npm install
npm run build
```

## Installation

### VS Code

1. Build the language server (`cargo build --release -p nwscript-lsp`)
2. Build the extension (`cd editors/vscode && npm install && npm run build`)
3. Copy the server binary to `editors/vscode/bin/`
4. Package the extension: `cd editors/vscode && npx vsce package`
5. Install the `.vsix` file in VS Code

Or for development, set `nwscriptLsp.serverPath` in VS Code settings to point to the built binary.

### Configuration

| Setting | Description | Default |
|---------|-------------|---------|
| `nwscriptLsp.serverPath` | Path to `nwscript-lsp` binary | Bundled binary |
| `nwscriptLsp.compilerPath` | Path to `nwn_script_comp` binary | Bundled binary |
| `nwscriptLsp.includeDirs` | Additional include directories | `[]` |

## Roadmap

- [ ] Cross-file symbol resolution (follow `#include` graph)
- [ ] Workspace-wide indexing
- [ ] Signature help
- [ ] Find references / rename
- [ ] Read `nasher.cfg` for include paths
- [ ] Semantic tokens (syntax highlighting from AST)
- [ ] Code actions (auto-import, unused variable removal)

## License

MIT
