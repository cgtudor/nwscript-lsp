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
        hover.rs       # Hover info: workspace symbols + local variables/parameters
        references.rs  # Find all references (whole-word search, skips comments/strings)
        semantic_tokens.rs # AST-based semantic highlighting (functions, types, params, etc.)
        signature.rs   # Signature help (parameter hints)
        symbols.rs     # Document symbols outline
        workspace_symbols.rs # Workspace-wide symbol search (Ctrl+T)
        inlay_hints.rs # Parameter name hints at call sites
        folding.rs     # Folding ranges (functions, structs, blocks, includes, comments)
        actions.rs     # Code actions (remove unused imports, remove unused variables/functions)
        refactor.rs    # Refactoring code actions (extract variable/function/to file)
        code_lens.rs   # Reference counts above functions, structs, and global vars/constants
        document_links.rs # Clickable #include directives
editors/
  vscode/          # VS Code extension (thin TypeScript client)
    src/
      extension.ts           # Extension entry point
      nuiPreview.ts          # NUI preview panel (webview lifecycle, re-solve on scale/view change)
      nwscript/
        interpreter.ts       # NWScript interpreter (evaluates NUI code, intercepts NUI calls)
        layoutSolver.ts      # NUI layout solver (Cassowary approximation with scale support)
        nui-builtins.ts      # NUI function implementations (NuiCol, NuiRow, NuiList, etc.)
        includeResolver.ts   # #include resolution for NUI preview
      webview/
        previewHtml.ts       # Preview webview (HTML/CSS/JS with scale/resolution/view controls)
    syntaxes/nwscript.tmLanguage.json  # TextMate grammar for syntax highlighting
bin/
  win64/           # Bundled nwn_script_comp.exe compiler binary
```

## Build Commands

```bash
# Check compilation (fast)
cargo check

# Run all tests (107 tests: lexer, parser, formatter, providers, refactor, nasher, integration)
cargo test

# Build release binary (Windows)
cargo build --release -p nwscript-lsp

# Build release binary (Linux cross-compile from Windows)
# Option A: cargo-zigbuild (zig.cmd wrapper at C:\Users\User\.local\bin\zig.cmd)
cargo zigbuild --release -p nwscript-lsp --target x86_64-unknown-linux-gnu
# Option B: WSL
wsl bash -c 'cd /mnt/d/tdn/workspace/nwscript-lsp && cargo build --release -p nwscript-lsp'

# Package VS Code extension (requires npm install first)
cd editors/vscode
npm install
cp ../../target/release/nwscript-lsp.exe bin/
npm run package:win      # esbuild bundle + platform-specific VSIX
# For Linux: swap bin/ to Linux binaries, then: npm run package:linux

# Publish both platform VSIXes
npx @vscode/vsce publish --packagePath nwscript-lsp-win32-x64-VERSION.vsix nwscript-lsp-linux-x64-VERSION.vsix --allow-missing-repository
```

### Cross-compilation notes
- **Linux nwn_script_comp** (bundled compiler): sourced from `D:/tdn/workspace/nwscript-ee-language-server/server/resources/compiler/linux/nwn_script_comp`
- **Windows nwn_script_comp**: at `bin/win64/nwn_script_comp.exe`
- **zigbuild**: `cargo-zigbuild` is installed; zig itself is pip-installed (`ziglang` package) with a `.cmd` wrapper at `C:\Users\User\.local\bin\zig.cmd` that delegates to `C:\Users\User\AppData\Roaming\Python\Python313\site-packages\ziglang\zig.exe`
- **Use `/publish` slash command** for the full build + package + publish workflow

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
- **Include-file errors** — when the compiler reports an error from an included file (e.g. `_tdn_handlefeats.nss(101)`), the diagnostic is placed at line 1 of the compiled file with the origin in the message (e.g. `INVALID DECLARATION TYPE (in _tdn_handlefeats.nss:101)`). This prevents errors from landing on unrelated lines when the line number belongs to a different file

### Auto-Import (completion.rs)
- Completion shows ALL workspace symbols (~2700+), not just those from the current include tree
- Symbols from non-included files get `additional_text_edits` that insert `#include` after the last existing import
- Each SymbolInfo tracks `include_name` (file stem) for determining what to import
- **Performance**: uses `for_each_symbol()` to iterate all workspace symbols by reference (no mass clone). The include tree is pre-computed once via `include_tree_set()` — an O(1) HashSet lookup per symbol instead of a recursive tree walk

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

Inlay hints settings under `nwscriptLsp.inlayHints.*`: `enabled` (true), `suppressForSingleArgCalls` (false)

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

### Inlay Hints (inlay_hints.rs)
- Walks the AST to find `Expr::Call` nodes, resolves callee name to get parameter names
- Emits `InlayHint` with `InlayHintKind::PARAMETER` at the start of each argument span
- **Suppression**: skips hints when the argument identifier matches the parameter name (case-insensitive), avoiding redundant hints like `Foo(nCount:` nCount`)`
- **Performance**: builds a `HashMap<&str, &SymbolInfo>` from `visible_symbols()` once at the start, then does O(1) lookups per call site. Previously called `find_symbol()` per call which walked the entire include tree each time — O(N * include_tree) for N call sites
- Configurable: `enabled` (default true), `suppressForSingleArgCalls` (default false)
- Settings flow from VS Code → `initializationOptions.inlayHints` → `InlayHintsSettings` in `NwscriptConfig`

### Workspace Symbol Search (workspace_symbols.rs)
- Implements `workspace/symbol` — the `Ctrl+T` / `#symbol` experience
- Case-insensitive substring matching against `index.all_workspace_symbols()`
- Prefix matches sorted first, then by name length; capped at 200 results
- Empty query returns nothing to avoid flooding the client

### Folding Ranges (folding.rs)
- **AST-based**: function bodies, struct bodies, if/else/while/for/switch/do-while blocks
- **Import folding**: consecutive `#include` groups (2+) with `FoldingRangeKind::Imports`
- **Comment folding**: block comments (`/* */`) and consecutive line comment groups via `Lexer::tokenize()` token scan
- Only multi-line spans produce fold ranges

### Document Links (document_links.rs)
- Makes `#include "filename"` directives Ctrl+Click-able
- Uses `IncludeDecl.path_span` for the link range and `index.resolve_include()` for the target URI

### Code Lens (code_lens.rs)
- Shows "N references" above function definitions, struct declarations, and global variable/constant declarations
- **Batch reference counting** — collects all symbol names in the file, then scans the workspace once for all of them via `count_references_batch()`. This is O(total_source) regardless of how many functions the file has, vs. O(N * total_source) for individual counting
- **Pre-resolved** — lenses are returned with commands already filled in from `textDocument/codeLens`, so `codeLens/resolve` is a no-op
- Subtracts declaration count from the total for actual usage count
- Clicking triggers VS Code's built-in `editor.action.findReferences` command

### Unused Variable Detection (actions.rs)
- Walks each function body collecting all `Stmt::VarDecl` declarations and function parameters
- For each, counts whole-word occurrences in the function body (skipping strings/comments)
- Variables with only 1 occurrence (the declaration itself) or 0 (parameters) are flagged unused
- Variables prefixed with `_` are exempt (intentionally unused convention)
- Produces `DiagnosticTag::UNNECESSARY` hints (grayed out) + quickfix code actions to remove variable declarations
- Parameters get diagnostics but no removal quickfix (removing a param breaks the function signature)

### Refactoring Code Actions (refactor.rs)
- **Extract Variable** — works with cursor position OR selection. Cursor on a function name like `StringToInt` extracts the whole call; cursor on a bare identifier walks up to the parent compound expression. Selection extracts exactly what's selected. Infers type from AST context (function return types, literal types, operator result types, local variable types). Inserts declaration before the containing statement with correct indentation. Correctly scopes into nested blocks (if/else-if/while/for/switch). Triggers rename after extraction via `nwscript-lsp.renameSymbol` extension command
- **Extract Function** — select one or more statements (works inside nested blocks). Finds the innermost block containing the selection via `find_innermost_block`. Detects free variables by walking the entire function body for declarations before the selection offset (not just the immediate block). Passes them as function parameters. Handles return statements by using the enclosing function's return type. Triggers rename after extraction
- **Extract to File** — cursor inside a function definition moves it to a new file. Uses `DocumentChanges::Operations` with `CreateFile` + `TextDocumentEdit` to properly create the new file. Copies all `#include` directives, removes the function (and its prototype if present), adds an `#include` for the new file. Respects the 16-character resref limit. Triggers file rename in explorer via `nwscript-lsp.renameFile` extension command
- All three are `CodeActionKind::REFACTOR_EXTRACT`, computed on-demand (not cached)
- Advertised via `CodeActionProviderCapability::Options` with `QUICKFIX` and `REFACTOR_EXTRACT` kinds
- **Extension commands**: `nwscript-lsp.renameSymbol` (finds symbol by name, positions cursor, triggers rename) and `nwscript-lsp.renameFile` (reveals file in explorer, triggers file rename)

### File Rename Support (server.rs — will_rename_files)
- Handles `workspace/willRenameFiles` for `.nss` files
- When a `.nss` file is renamed (e.g. via Extract to File's rename flow or manually), scans all indexed files for `#include "old_name"` and returns a `WorkspaceEdit` replacing them with `#include "new_name"`
- Registered via `WorkspaceServerCapabilities.file_operations.will_rename` with glob `**/*.nss`

### NUI Preview (editors/vscode/)
- **Interpreter** (`interpreter.ts`) — evaluates NWScript to produce NUI window JSON. Intercepts `NuiCreate`, `NuiWindow`, `NuiSetBind` (geometry capture), and `NuiSetGroupLayout` (view switching). Detects view-builder functions by probing 0-param functions that return NUI layout JSON
- **Layout solver** (`layoutSolver.ts`) — approximates NWN:EE's kiwi/Cassowary layout engine. Key constants: `GAP=8` (unscaled default margin spacing), `BODY_PAD_X=10`, `BODY_PAD_Y=16` (unscaled Nuklear window padding), `TITLE_BAR_H=28` (scales with UI), `DEFAULT_RIGID_W=150` (logical units, scales with UI). Scale affects: window dimensions, title bar, NuiWidth/NuiHeight, NuiPadding, row_height, default widget widths. Scale does NOT affect: GAP, BODY_PAD, NuiMargin (known engine bug)
- **Rigid vs flexible sizing** (calibrated against in-game dm_menu_nui screenshots at UI scale 1.0 and 2.2): button-family widgets (button, button_select, button_image, combo) without explicit NuiWidth get a STRONG ~150-unit default — they never stretch or shrink; rows of them overflow + scrollbar instead. Textedit/label/spacer are genuinely flexible. **NuiGroup content does NOT fill the group pane**: it solves at its natural width (widest rigid row inside); flexible elements stretch only up to that width; rigid-only rows narrower than it stay natural, left-aligned. The window root fills the pane but rigid content overflows it (h-scrollbar on the body, `contentWidth` prop → `min-width`)
- **Webview** (`previewHtml.ts`) — renders solved layout as absolutely-positioned HTML elements inside a scrollable window body. Controls: function selector, screen resolution, UI scale (dynamically populated from `min(w/900,h/700)` formula), fit mode (Window/Screen), view switcher
- **Preview panel** (`nuiPreview.ts`) — manages webview lifecycle, handles re-solve requests (scale changes, view switches). Stores interpreter instance for view switching without re-evaluation
- **Scale behavior** — verified via ReVa + community reports: element sizes scale, margins don't, list cell spacing doesn't. Unscaled overhead (GAP+padding) becomes proportionally smaller at higher scales, explaining why tight layouts overflow at 1.0x but fit at 2.2x

### Initializer Value Hover
- `SymbolInfo.initializer_text` stores the raw expression text from `VarDecl.initializer`
- Hover for constants shows `const int NAME = VALUE` instead of just `const int NAME`
- Hover for global variables also shows initializer values (e.g. `int TRUE = 1` for nwscript.nss constants that lack the `const` keyword)

## Important Notes

- **Nasher cache is critical** — compiler diagnostics depend on `.nasher/cache/<target>/` existing. If the user hasn't run `nasher compile` yet, the cache won't exist and compiler diagnostics will fall back to multi-directory `--dirs` which can produce false positives
- **nwscript.nss special handling** — it's the only file searched outside normal source dirs. It's also the only file implicitly included in every file's visible symbols
- **`is_definition` flag** — SymbolInfo tracks whether a function has a body. Goto-definition always prefers implementations over forward declarations
- **Diagnostic merging** — parser diagnostics (on keystroke), compiler diagnostics (on save), unused-import hints, and unused-variable hints are all merged and published together. Compiler diags are cleared on edit to prevent stale errors
- **Unused function analysis is deferred** — unlike import/variable analysis (fast, file-local), unused function detection requires a full workspace scan. It runs on `did_open` (after initial diagnostics publish) and `did_save`, with results cached in `unused_fn_diags`/`unused_fn_actions` DashMaps. Cache is cleared on edit so stale hints disappear until next save. Uses `count_references_batch()` for O(total_source) instead of O(N * total_source)
- **Code action caching** — import and variable analysis results are cached in `cached_actions` DashMap during `publish_diagnostics_for`. The `code_action` handler reads from cache instead of recomputing, making lightbulb/quickfix responses instant
- **did_change is lightweight** — only processes the changed file. Does NOT loop over other open documents (rename already sends `didChange` per affected file via WorkspaceEdit)
- **Batch reference counting** — `count_references_batch()` in references.rs scans each file once for all symbol names via single-pass identifier extraction with `HashSet` lookup. Used by code lens and unused function detection. Complexity is O(total_source_size) regardless of how many symbols are being counted
