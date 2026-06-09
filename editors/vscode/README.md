# NWScript Language Server

Language server for **NWScript**, the scripting language used by Neverwinter Nights: Enhanced Edition. Provides IDE features for `.nss` files in VS Code.

## Features

- **Completions** -- all symbols across your workspace with auto-import, plus local variables and parameters (sorted by relevance)
- **Go to Definition** -- jump to function/constant/struct definitions (prefers implementations over forward declarations)
- **Find References** -- find all usages of a symbol across the workspace
- **Rename Symbol** -- rename functions, constants, and local variables across all files
- **Hover** -- type signatures, doc comments, and source file for functions, structs, constants, variables, and local variables/parameters
- **Signature Help** -- parameter hints as you type function calls
- **Diagnostics** -- real-time parser errors on keystroke, compiler errors on save (via `nwn_script_comp`)
- **Unused Import Detection** -- grayed-out `#include` directives that aren't referenced, with quick-fix removal
- **Inlay Hints** -- parameter name hints at call sites (e.g. `nObjectType: 1, sTemplate: "goblin01"`)
- **Workspace Symbol Search** -- find any symbol across the workspace with `Ctrl+T`
- **Document Symbols** -- outline view of functions, structs, and constants
- **Folding Ranges** -- collapse functions, structs, control flow blocks, `#include` groups, and comment blocks
- **Code Formatting** -- full document, range, and on-type formatting with configurable style
- **Semantic Highlighting** -- AST-based highlighting for function calls, parameters, struct names, and field access
- **Syntax Highlighting** -- TextMate grammar for NWScript

## Setup

1. Install the extension
2. Open a folder containing `.nss` files

The LSP automatically discovers source directories from `nasher.cfg` if present. If your project doesn't use Nasher, it indexes the workspace root.

### Vanilla Scripts and Engine Built-ins

The LSP automatically finds your NWN:EE installation (via `NWN_ROOT` env var, Steam, Beamdog, or GOG) and extracts all vanilla `.nss` scripts from the game's KEY/BIF files. This means:

- `#include "nw_i0_generic"` and other vanilla includes resolve automatically
- Go-to-definition works on vanilla functions (opens the extracted source)
- `nwscript.nss` (engine built-in definitions) is included automatically
- Your workspace files always take priority over vanilla scripts

If auto-detection doesn't find your installation, set it explicitly:

```json
"nwscriptLsp.nwnRoot": "C:/Program Files (x86)/Steam/steamapps/common/Neverwinter Nights"
```

If you don't need vanilla scripts and only want `nwscript.nss` (engine built-ins), you can disable the extraction for faster startup:

```json
"nwscriptLsp.extractVanillaScripts": false
```

### Compiler Diagnostics

For on-save compiler diagnostics, the extension needs `nwn_script_comp` (the NWN script compiler). It looks for a bundled copy next to the extension binary, then falls back to `PATH`. You can also set an explicit path:

```json
"nwscriptLsp.compilerPath": "C:/path/to/nwn_script_comp.exe"
```

For best results with Nasher-based projects, run `nasher compile` at least once so the `.nasher/cache/` directory exists -- the LSP compiles against this cache to avoid false positives.

## Settings

All settings are under the `nwscriptLsp` namespace. Open **Settings** (Ctrl+,) and search for "nwscript" to see them in the UI, or click the links below to jump directly to each setting.

### General

| Setting | Default | Description |
|---------|---------|-------------|
| `compilerPath` | `""` | Path to `nwn_script_comp`. Empty = bundled or PATH. |
| `serverPath` | `""` | Path to `nwscript-lsp` binary. Empty = bundled or PATH. |
| `nwnRoot` | `""` | Path to NWN:EE installation. Empty = auto-detect from env/Steam/Beamdog/GOG. |
| `nwscriptNssPath` | `""` | Path to `nwscript.nss`. Empty = extracted from NWN install or searched in workspace. |
| `extractVanillaScripts` | `true` | Extract vanilla `.nss` scripts from NWN:EE KEY/BIF files. Set to `false` to skip extraction if you only need `nwscript.nss`. |
| `includeDirs` | `[]` | Additional source directories (added to those from `nasher.cfg`). |
| `excludeDirs` | `["node_modules", "target", "build", "output"]` | Directory names to skip when scanning for `.nss` files. Dot-prefixed directories (`.git`, `.nasher`, etc.) are always skipped. |

### Inlay Hints

| Setting | Default | Description |
|---------|---------|-------------|
| `inlayHints.enabled` | `true` | Show parameter name inlay hints at call sites. Also requires VS Code's `editor.inlayHints.enabled`. |
| `inlayHints.suppressForSingleArgCalls` | `false` | Hide inlay hints for function calls with only one argument. |

### Formatter

The extension registers itself as the default formatter for NWScript and enables format-on-save automatically. To disable formatting entirely, add to your settings:

```json
"[nwscript]": {
    "editor.formatOnSave": false,
    "editor.defaultFormatter": null
}
```

To enable format-on-type (auto-formats as you type `}`, `;`, and `Enter`):

```json
"[nwscript]": {
    "editor.formatOnType": true
}
```

| Setting | Default | Description |
|---------|---------|-------------|
| `formatter.braceStyle` | `"nextLine"` | `"nextLine"` (Allman) or `"sameLine"` (K&R) |
| `formatter.maxLineWidth` | `120` | Line width before wrapping function params to one-per-line |
| `formatter.maxBlankLines` | `1` | Max consecutive blank lines (extras are collapsed) |
| `formatter.sortIncludes` | `true` | Sort `#include` directives alphabetically |
| `formatter.trimTrailingWhitespace` | `true` | Remove trailing whitespace |
| `formatter.spaceAfterKeywords` | `true` | `if (x)` vs `if(x)` |
| `formatter.spaceInsideParens` | `false` | `( x )` vs `(x)` |
| `formatter.spaceAroundOperators` | `true` | `a + b` vs `a+b` |
| `formatter.spaceAfterComma` | `true` | `f(a, b)` vs `f(a,b)` |

## Project Structure Support

The LSP works with any NWScript project layout:

- **Nasher projects** -- automatically reads `nasher.cfg` (in workspace root and immediate subdirectories) to discover source directories
- **Multi-root workspaces** -- scans all workspace folders
- **Plain folders** -- if no `nasher.cfg` is found, indexes the workspace root directly
- **Extra include dirs** -- use `includeDirs` to add directories outside the workspace (e.g., shared libraries)
