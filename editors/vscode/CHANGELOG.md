# Changelog

## 1.6.1

### Fixes

- **NUI Preview** -- The preview now takes account of a few more pecularities with the way the engine resolves layout constraints, providing a more accurate portrayal of element widths.
- **NUI Preview** -- Fixed a "Maximum loop iterations exceeded" error (and the resulting garbled preview) caused by `GetFirst*`/`GetNext*` iterator loops, such as `GetFirstPC`/`GetNextPC`, never terminating. Object enumeration loops now run correctly, so previews for windows that list players or other objects render properly.

## 1.6.0

### Fixes

- **Linux: auto-set execute permission** -- the bundled LSP binary now gets `chmod +x` automatically on activation, so Linux users no longer need to manually set execute permissions after install.

### New Features

- **(EXPERIMENTAL) NUI Preview** -- Files that are recognized to contain NUI windows now have the option to open a preview that shows approximately what the layout of the built NUI window will look like. Features include:
  - Layout solver approximating NWN:EE's kiwi/Cassowary constraint engine
  - Screen resolution and UI scale simulation with resolution-aware scale options matching the engine's formula
  - Scrollable window body when content overflows (matching in-game behavior)
  - View switching for NUIs with swappable group layouts (tabs, multi-view windows)
  - Fairly accurate rendering of rows, columns, groups, lists, buttons, labels, text edits, and more

## 1.5.0

### New Features

- **Extract Variable** -- place your cursor on an expression (e.g. a function call like `StringToInt(...)`) or select one, then use the lightbulb (Ctrl+.) to extract it into a local variable. The type is inferred automatically. Triggers rename so you can immediately name the variable.
- **Extract Function** -- select one or more statements and extract them into a new function. Variables from outer scopes are automatically detected and passed as parameters. Works inside nested blocks (if/else-if/while/for/switch).
- **Extract to File** -- place your cursor inside a function and move it to a new file. Creates the file with all necessary `#include` directives, removes the function (and its prototype) from the current file, adds an `#include` for the new file, and opens file rename in the explorer so you can choose the filename.
- **Auto-update includes on file rename** -- renaming a `.nss` file in the explorer automatically updates all `#include` directives referencing it across the workspace.

### Improvements

- **Compiler diagnostics from included files** -- errors originating from included files (e.g. `_tdn_handlefeats.nss:101`) are now shown at the top of the compiled file with the source location in the message, instead of landing on an unrelated line number.

## 1.4.0

### New Features

- **Document links** -- `#include "filename"` directives are now clickable (Ctrl+Click) to open the resolved file.
- **Code lens** -- reference counts shown above function definitions, struct declarations, and global variable/constant declarations (e.g. "3 references"). Click to find all references.
- **Unused variable detection** -- local variables and parameters that are never used are grayed out with a hint diagnostic. Unused variable declarations have a quickfix action to remove them. Variables prefixed with `_` are exempt.
- **Unused function detection** -- functions that are never referenced anywhere in the workspace are grayed out with a quickfix to remove them. Entry points (`main`, `StartingConditional`) and `_`-prefixed functions are exempt.
- **Initializer value hover** -- hovering a constant or global variable now shows its initializer value (e.g. `const int DURATION_TYPE_INSTANT = 0`, `int TRUE = 1`).

### Performance

- **Batch reference counting** -- code lens and unused function detection now scan the workspace once for all symbols (O(total_source)) instead of once per symbol (O(N * total_source)). Dramatically faster for files with many functions.
- **Inlay hints** -- builds a symbol lookup map once per request instead of walking the include tree per function call. Large files (1000+ lines) are now near-instant.
- **Completion** -- iterates workspace symbols by reference (no mass clone) and pre-computes the include tree set once.
- **Code actions** -- import and variable analysis results are cached during diagnostic publishing. The lightbulb/quickfix menu opens instantly instead of recomputing.
- **File open** -- initial diagnostics (parser errors, imports, variables) publish immediately. Unused function analysis runs after and publishes a second update.
- **Editing** -- `did_change` only processes the edited file, not all open documents.

## 1.3.0

### New Features

- **Inlay hints** -- parameter name hints at call sites (e.g. `CreateObject(nObjectType: 1, sTemplate: "goblin01", ...)`). Suppresses redundant hints when the argument name matches the parameter name. Configurable via `inlayHints.enabled` and `inlayHints.suppressForSingleArgCalls` settings.
- **Workspace symbol search** -- find any function, struct, constant, or variable across the entire workspace with `Ctrl+T` (or `#` in the command palette). Case-insensitive substring matching with prefix matches ranked first.
- **Folding ranges** -- collapse function bodies, struct bodies, control flow blocks (if/while/for/switch), consecutive `#include` groups, block comments, and consecutive line comment groups in the editor.

### Improvements

- **Hover on local variables and parameters** -- hovering over a local variable or function parameter now shows its type and kind (e.g. `(parameter) object oPC`, `(local) int nCount`). Previously only workspace-level symbols had hover info.
- **Improved function hover** -- function hover now shows parameter default values (e.g. `int bIncludeDead = FALSE`) and the source file name. Removed the `{...}` noise from function bodies.
- **Improved symbol hover** -- all symbol hovers (constants, global variables, structs) now show the source file they are defined in.
- **Subtler inlay hint styling** -- parameter hints now use a transparent background and faded text color so they blend in rather than standing out as boxes.

### New Settings

- **`inlayHints.enabled`** -- enable/disable parameter name inlay hints (default: `true`)
- **`inlayHints.suppressForSingleArgCalls`** -- hide hints for single-argument function calls (default: `false`)

## 1.2.1

### Fixes

- Linux release now bundles `nwn_script_comp` for compiler diagnostics (was missing in 1.2.0)

## 1.2.0

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
