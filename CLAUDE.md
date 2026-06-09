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
      keybif.rs    # KEY/BIF file reader for extracting vanilla scripts from NWN install
      nwn_install.rs # NWN:EE installation auto-detection (Steam, Beamdog, GOG, env var)
      providers/
        completion.rs  # Completion with auto-import + local variable/parameter completion
        definition.rs  # Goto-definition (prefers implementations over prototypes)
        hover.rs       # Hover info with doc comment extraction
        references.rs  # Find all references (whole-word search, skips comments/strings)
        semantic_tokens.rs # AST-based semantic highlighting (functions, types, params, etc.)
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

# Run all tests (57 tests: lexer, parser, formatter, signature, nasher, integration)
cargo test

# Build release binary
cargo build --release -p nwscript-lsp

# Package VS Code extension (requires npm install first)
cd editors/vscode
npm install
npm run package          # esbuild bundle + minify
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
- **Implicit nwscript.nss** — engine built-in functions are always visible even without `#include`. Auto-discovered via recursive search from workspace roots, or set explicitly via `nwscriptNssPath` config
- **URI normalization** — all URI lookups go through `normalize_uri()` to fix Windows drive letter case mismatch (`d:` vs `D:`)
- **Latin-1 fallback** — files that fail UTF-8 decoding are read as Latin-1 (BioWare-era scripts use Windows-1252)
- **Configurable directory exclusion** — dot-prefixed directories are always skipped. Additional names come from the `excludeDirs` config setting (defaults: `node_modules`, `target`, `build`, `output`). The `nwscript.nss` auto-discovery search ignores the exclude list so it can find the file in directories like `docs/`

### Vanilla Script Extraction (keybif.rs, nwn_install.rs)
- **NWN installation auto-detection** — checks `nwnRoot` config, then `NWN_ROOT` env var, then Steam/Beamdog/GOG common paths (platform-specific)
- **KEY/BIF reading** — parses all KEY files in `<nwn_root>/data/` (`nwn_base.key`, `nwn_base_loc.key`, `nwn_retail.key`, `nwn_retail_loc.key`), resolves BIF references, extracts all ResType 2009 (.nss) resources
- **V1 and E1 format support** — handles both original NWN (V1) and Enhanced Edition (E1) formats. E1 adds optional CompressedBuf wrapper with zstd/zlib decompression
- **Cache directory** — extracted vanilla `.nss` files are written to `<cache_dir>/nwscript-lsp/vanilla/`. On Windows this is `%LOCALAPPDATA%/nwscript-lsp/vanilla/`
- **Override priority** — vanilla scripts are indexed FIRST (phase 1), then workspace files (phase 2). Since `include_map` uses last-write-wins, workspace files naturally override vanilla. This matches the game engine's override behavior
- **Go-to-definition** — jumping to a vanilla function opens the cached extracted file, similar to how C# IDEs show decompiled library sources

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

### Formatter (crates/parser/src/formatter/)
- **AST-based reprinting** — walks the parsed AST and emits freshly formatted output
- **Comment preservation** — uses a token cursor that walks the lexer's token stream in parallel with the AST walk. Comments are classified as trailing (same line as previous code) or leading (own line) based on whether a newline was seen. The key insight: `collect_comments_before()` initializes `seen_newline` from `self.at_line_start` so comments at file start or after a newline are correctly treated as leading, not trailing
- **Token cursor design** — non-trivia tokens are skipped (not break the loop) in `collect_comments_before()`, so the cursor always advances forward keeping pace with the AST walk. Without this, the cursor gets stuck at the first non-trivia token and all subsequent comment collection fails
- **Include sorting** — collects all `#include` declarations with their associated comments, sorts by path (case-insensitive), then emits. Comments travel with their include
- **Line wrapping** — function params and call args wrap to one-per-line with continuation indent when they exceed `max_line_width`
- **Blank line preservation** — `TriviaResult` tracks `had_blank_lines` from the token scan (not just from comment metadata) so blank lines between statements are preserved even without comments
- **C#-style rules** — Allman braces (default), 1 blank line between top-level decls, no blank line after `{` or before `}`, braceless `if`/`while`/etc. get braces enforced
- **Expression formatting** — `format_expr_str()` returns a String (not writing to output) so line-length decisions can be made before committing. Original literal text is preserved via spans (keeps hex notation, float format, etc.)
- **Idempotent** — formatting already-formatted code produces identical output (tested)

#### Formatter Architecture
```
crates/parser/src/formatter/
  mod.rs       # FormatConfig, BraceStyle, public format() API
  printer.rs   # Printer struct — AST walker + token cursor
  tests.rs     # 33 tests covering all constructs

crates/lsp/src/providers/
  formatting.rs  # LSP integration: document/range/on-type formatting, FormatterSettings
```

#### LSP Formatting Capabilities
- `textDocument/formatting` — full document reformat
- `textDocument/rangeFormatting` — formats whole document (standard approach)
- `textDocument/onTypeFormatting` — triggers on `}` (re-indent), `;` (re-indent), `\n` (auto-indent)
- Configuration flows from VS Code settings → `initializationOptions.formatter` → `FormatterSettings` → `FormatConfig`

#### VS Code Settings
General settings: `compilerPath`, `serverPath`, `nwnRoot` (NWN:EE install path, auto-detected), `nwscriptNssPath`, `includeDirs`, `excludeDirs` (defaults: `["node_modules", "target", "build", "output"]`)

Formatter settings under `nwscriptLsp.formatter.*`: `maxLineWidth` (120), `braceStyle` (nextLine/sameLine), `sortIncludes` (true), `maxBlankLines` (1), `trimTrailingWhitespace` (true), `spaceAfterKeywords` (true), `spaceInsideParens` (false), `spaceAroundOperators` (true), `spaceAfterComma` (true)

Users should also set in their VS Code settings:
```json
{
    "[nwscript]": {
        "editor.formatOnSave": true,
        "editor.formatOnType": true,
        "editor.defaultFormatter": "krezk.nwscript-lsp"
    }
}
```

## Important Notes

- **Nasher cache is critical** — compiler diagnostics depend on `.nasher/cache/<target>/` existing. If the user hasn't run `nasher compile` yet, the cache won't exist and compiler diagnostics will fall back to multi-directory `--dirs` which can produce false positives
- **nwscript.nss special handling** — it's the only file searched outside normal source dirs. It's also the only file implicitly included in every file's visible symbols
- **`is_definition` flag** — SymbolInfo tracks whether a function has a body. Goto-definition always prefers implementations over forward declarations
- **Diagnostic merging** — parser diagnostics (on keystroke), compiler diagnostics (on save), and unused-import hints are all merged and published together. Compiler diags are cleared on edit to prevent stale errors
