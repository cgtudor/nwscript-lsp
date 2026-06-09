# Changelog

## 1.1.1

### New Settings

- **`extractVanillaScripts`** -- new boolean setting (default: `true`) to disable extraction of vanilla `.nss` scripts from KEY/BIF files. Users who only need `nwscript.nss` can set this to `false` for faster startup.

## 1.1.0

### New Features

- **Local variable completion** -- function parameters and local variables now appear in completions, sorted above workspace symbols
- **Find References** -- find all usages of a symbol across the workspace (Shift+F12)
- **Rename Symbol** -- rename functions, constants, and local variables across all files (F2)
- **Semantic highlighting** -- AST-based highlighting for function calls, parameters, struct names, and field access
- **Completion sorting** -- local variables > imported symbols > auto-import symbols

### Improvements

- **Compiler diagnostics** -- now passes `--root` and `--userdirectory` to the compiler, matching nasher's behavior and fixing false "variable defined without type" errors
- **Cross-file diagnostic refresh** -- after rename or edits, diagnostics for all open files are refreshed immediately
- **Find references performance** -- fast substring pre-check skips files that don't contain the symbol

### Formatter Fixes

- Consecutive variable declarations and function prototypes now preserve the user's blank line grouping instead of forcing a blank line between each one
- Fixed extra blank lines being inserted between commented prototypes/statements
- Trailing comments on inline if/while/for statements (e.g., `if (x) y = 1; // comment`) are moved inside the block when braces are added, instead of being orphaned after `}`

## 1.0.0

Initial release.

- **Completions** -- all symbols across the workspace with auto-import
- **Go to Definition** -- cross-file, prefers implementations over forward declarations
- **Hover** -- type signatures and doc comments
- **Signature Help** -- parameter hints on `(` and `,`
- **Diagnostics** -- real-time parser errors, compiler errors on save
- **Unused Import Detection** -- grayed-out unused `#include` with quick-fix removal
- **Document Symbols** -- outline view
- **Code Formatting** -- Allman/K&R braces, include sorting, line wrapping, comment preservation
- **Syntax Highlighting** -- TextMate grammar
- **Vanilla Script Support** -- auto-extracts `.nss` files from NWN:EE KEY/BIF for include resolution and go-to-definition
- **NWN Install Auto-Detection** -- finds NWN:EE via `NWN_ROOT` env var, Steam, Beamdog, or GOG
- **Nasher Integration** -- reads `nasher.cfg` for source directory discovery
