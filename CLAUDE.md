# CLAUDE.md

## Project Overview

NWScript Language Server — an LSP implementation for the NWScript scripting language (Neverwinter Nights: Enhanced Edition). Written in Rust with a hand-written parser and tower-lsp framework. Includes a VS Code extension.

## Architecture

```
crates/
  parser/          # Zero-dependency NWScript parser library
    src/
      lexer.rs     # Hand-written lexer, produces all tokens including trivia
      token.rs     # Token kinds (keywords, operators, literals, trivia)
      ast.rs       # Full AST types (declarations, statements, expressions)
      parser.rs    # Recursive descent parser with Pratt expression parsing
      span.rs      # Byte-offset spans and line index for LSP position conversion
  lsp/             # Language server binary
    src/
      server.rs    # tower-lsp LanguageServer impl, wires everything together
      index.rs     # Workspace-wide symbol index, include graph, cross-file resolution
      document.rs  # Open document tracking
      diagnostics.rs  # External compiler (nwn_script_comp) integration
      nasher.rs    # nasher.cfg parser for source directory discovery
      providers/
        completion.rs  # Completion with auto-import (all workspace symbols)
        definition.rs  # Goto-definition (prefers implementations over prototypes)
        hover.rs       # Hover info with doc comment extraction
        signature.rs   # Signature help (parameter hints)
        symbols.rs     # Document symbols outline
        actions.rs     # Code actions (remove unused imports)
editors/
  vscode/          # VS Code extension (thin TypeScript client)
    src/extension.ts
    syntaxes/nwscript.tmLanguage.json  # TextMate grammar for syntax highlighting
bin/
  win64/           # Bundled nwn_script_comp.exe compiler binary
```

## Build Commands

```bash
# Check compilation (fast)
cargo check

# Run all tests (24 tests: lexer, parser, signature, nasher, integration)
cargo test

# Build release binary
cargo build --release -p nwscript-lsp

# Package VS Code extension (requires npm install first)
cd editors/vscode
npm install
npx tsc -p ./
cp ../../target/release/nwscript-lsp.exe bin/
npx @vscode/vsce package --allow-missing-repository
```

## Key Design Decisions

### Parser (crates/parser/)
- **Hand-written lexer** — produces ALL tokens including trivia (whitespace, comments) so the parser can skip them while preserving comment info for hover docs
- **Recursive descent + Pratt** — recursive descent for declarations/statements, Pratt parser for expressions with correct operator precedence
- **Error recovery** — on parse error, skips to synchronization points (`;`, `}`, declaration keywords), always produces partial ASTs from broken code
- **Comma-separated var decls** — `string a, b = "x", c;` produces multiple VarDecl nodes via `pending_stmts` mechanism
- **Zero dependencies** — the parser crate has no external deps

### Workspace Index (index.rs)
- **Implicit nwscript.nss** — engine built-in functions are always visible even without `#include`. Found via recursive search from workspace roots (lives in `docs/nwn_source/`)
- **URI normalization** — all URI lookups go through `normalize_uri()` to fix Windows drive letter case mismatch (`d:` vs `D:`)
- **Latin-1 fallback** — files that fail UTF-8 decoding are read as Latin-1 (BioWare-era scripts use Windows-1252)
- **Directory skip list** — `docs/`, `nwn_source/`, `.hidden/`, `node_modules/`, `target/` are skipped during scanning to avoid indexing reference material as source

### Compiler Diagnostics (diagnostics.rs)
- **Nasher cache compilation** — writes current source INTO the nasher cache (`.nasher/cache/<target>/`), compiles from there, then restores the original. This matches how nasher itself compiles and ensures the compiler's resman resolves workspace overrides correctly over NWN key/bif files
- **Why not temp files** — the compiler adds the compiled file's directory to resman at a priority that differs from `--dirs`. Compiling from outside the cache causes false duplicate-function errors
- **Flags** — `-s` (simulate), `-n` (no entry point), `-E` (all errors), `--quiet`, `--dirs <cache>`

### Auto-Import (completion.rs)
- Completion shows ALL workspace symbols (~2700+), not just those from the current include tree
- Symbols from non-included files get `additional_text_edits` that insert `#include` after the last existing import
- Each SymbolInfo tracks `include_name` (file stem) for determining what to import

### Unused Import Detection (actions.rs)
- Scans all identifiers in the file (skipping comments and strings)
- For each `#include`, checks if any symbol from that file is referenced
- Unused imports get `DiagnosticTag::Unnecessary` (grayed out) + quickfix code action to remove

## Important Notes

- **Nasher cache is critical** — compiler diagnostics depend on `.nasher/cache/<target>/` existing. If the user hasn't run `nasher compile` yet, the cache won't exist and compiler diagnostics will fall back to multi-directory `--dirs` which can produce false positives
- **nwscript.nss special handling** — it's the only file searched outside normal source dirs. It's also the only file implicitly included in every file's visible symbols
- **`is_definition` flag** — SymbolInfo tracks whether a function has a body. Goto-definition always prefers implementations over forward declarations
- **Diagnostic merging** — parser diagnostics (on keystroke), compiler diagnostics (on save), and unused-import hints are all merged and published together. Compiler diags are cleared on edit to prevent stale errors
