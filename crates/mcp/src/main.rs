use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nwscript_lsp::index::{SymbolInfo, SymbolKind, WorkspaceIndex};
use nwscript_lsp::lsp_types::{Position, Url};
use nwscript_lsp::providers;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router, ServiceExt};
use tracing_subscriber::EnvFilter;

// =============================================================================
// MCP Server
// =============================================================================

#[derive(Clone)]
struct NwscriptMcp {
    index: Arc<WorkspaceIndex>,
}

impl NwscriptMcp {
    fn new(workspace_dir: &Path) -> Self {
        let source_dirs = nwscript_lsp::nasher::discover_source_dirs(workspace_dir);
        let exclude_dirs = nwscript_lsp::server::default_exclude_dirs();

        // Try to extract vanilla NWN scripts for full symbol coverage.
        let (vanilla_cache_dir, _nwn_root) =
            nwscript_lsp::server::extract_vanilla_scripts(None);

        let index = WorkspaceIndex::new(
            source_dirs,
            vec![workspace_dir.to_path_buf()],
            exclude_dirs,
        );
        index.scan_workspace(None, vanilla_cache_dir.as_deref());

        tracing::info!(
            "MCP index ready: {} files",
            index.all_files().len()
        );

        Self {
            index: Arc::new(index),
        }
    }

    /// Re-read a file from disk and update it in the index before querying.
    fn refresh_file(&self, path: &Path) -> Option<Url> {
        let source = read_file_lossy(path)?;
        let uri = Url::from_file_path(path).ok()?;
        self.index.update_file(&uri, source);
        Some(uri)
    }
}

/// Read a file with UTF-8 decoding, falling back to Latin-1 for legacy scripts.
fn read_file_lossy(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Some(s),
        Err(_) => Some(bytes.iter().map(|&b| b as char).collect()),
    }
}

/// Convert a file path string to an absolute PathBuf.
fn resolve_path(file_path: &str) -> PathBuf {
    let p = PathBuf::from(file_path);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

/// Format a symbol for display.
fn format_symbol(sym: &SymbolInfo) -> String {
    let kind = match sym.kind {
        SymbolKind::Function => "function",
        SymbolKind::Struct => "struct",
        SymbolKind::Constant => "constant",
        SymbolKind::Variable => "variable",
        SymbolKind::StructField => "field",
    };
    let file = sym
        .uri
        .to_file_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| sym.uri.to_string());
    let mut s = format!("[{}] {} -- {}", kind, sym.detail, file);
    if let Some(doc) = &sym.doc {
        let short = doc.lines().next().unwrap_or("");
        if !short.is_empty() {
            s.push_str(&format!("\n  {}", short));
        }
    }
    s
}

// =============================================================================
// Tool parameter types
// =============================================================================

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct FileParams {
    #[schemars(description = "Absolute or relative path to a .nss file")]
    file_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PositionParams {
    #[schemars(description = "Absolute or relative path to a .nss file")]
    file_path: String,
    #[schemars(description = "1-based line number")]
    line: u32,
    #[schemars(description = "1-based column number")]
    column: u32,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SymbolSearchParams {
    #[schemars(description = "Symbol name to search for (exact match, falls back to substring)")]
    name: String,
    #[schemars(description = "Optional file path for scoping the search to symbols visible from that file's include tree")]
    file_path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RenameParams {
    #[schemars(description = "Absolute or relative path to a .nss file containing the symbol")]
    file_path: String,
    #[schemars(description = "1-based line number of the symbol to rename")]
    line: u32,
    #[schemars(description = "1-based column number of the symbol to rename")]
    column: u32,
    #[schemars(description = "The new name for the symbol")]
    new_name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WorkspaceDiagnosticsParams {
    #[schemars(description = "If true, also include unused import hints (default: false)")]
    include_hints: Option<bool>,
}

// =============================================================================
// Tool implementations
// =============================================================================

#[tool_router(server_handler)]
impl NwscriptMcp {
    #[tool(description = "Parse a NWScript (.nss) file and return any syntax errors or warnings. Also reports unused #include directives.")]
    fn get_diagnostics(
        &self,
        Parameters(FileParams { file_path }): Parameters<FileParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let mut output = Vec::new();

        // Parser errors
        for err in &file.parsed.errors {
            let (line, col) = file.line_index.line_col(err.span.start);
            output.push(format!(
                "{}:{}:{}: error: {}",
                path.display(),
                line + 1,
                col + 1,
                err.message,
            ));
        }

        // Unused import hints
        let analysis = providers::actions::analyze_imports(
            &self.index, &uri, &file.parsed, &file.source, &file.line_index,
        );
        for diag in &analysis.diagnostics {
            output.push(format!(
                "{}:{}:{}: hint: {}",
                path.display(),
                diag.range.start.line + 1,
                diag.range.start.character + 1,
                diag.message,
            ));
        }

        if output.is_empty() {
            "No diagnostics -- file parses cleanly.".to_string()
        } else {
            output.join("\n")
        }
    }

    #[tool(description = "Get type signature and documentation for the NWScript symbol at a given file position. Returns the symbol's type, parameters (for functions), and any doc comments. Uses 1-based line and column numbers.")]
    fn get_hover(
        &self,
        Parameters(PositionParams {
            file_path,
            line,
            column,
        }): Parameters<PositionParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let line0 = line.saturating_sub(1);
        let col0 = column.saturating_sub(1);

        let Some(offset) = file.line_index.offset(line0, col0) else {
            return "Error: position out of range".to_string();
        };

        let Some(ident) = providers::hover::find_ident_at(&file.source, offset as usize) else {
            return "No identifier at position".to_string();
        };

        let symbols = self.index.visible_symbols(&uri);
        let Some(sym) = symbols.iter().find(|s| s.name == ident) else {
            return format!("Symbol '{}' not found in visible scope", ident);
        };

        let hover = providers::hover::hover_for_symbol(sym);
        match hover.contents {
            nwscript_lsp::lsp_types::HoverContents::Markup(m) => m.value,
            _ => "No hover info available".to_string(),
        }
    }

    #[tool(description = "Find the definition location of the NWScript symbol at a given file position. Returns the file path and line number where the symbol is defined. Prefers implementations over forward declarations. Uses 1-based line and column numbers.")]
    fn goto_definition(
        &self,
        Parameters(PositionParams {
            file_path,
            line,
            column,
        }): Parameters<PositionParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let line0 = line.saturating_sub(1);
        let col0 = column.saturating_sub(1);
        let position = Position::new(line0, col0);

        let Some(location) = providers::definition::goto_definition(
            &self.index,
            &file.source,
            &file.line_index,
            position,
            &uri,
        ) else {
            return "No definition found".to_string();
        };

        let def_path = location
            .uri
            .to_file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| location.uri.to_string());

        format!(
            "{}:{}:{}",
            def_path,
            location.range.start.line + 1,
            location.range.start.character + 1,
        )
    }

    #[tool(description = "Find all references to the NWScript symbol at a given file position across the entire workspace. Returns a list of file:line:column locations. Uses 1-based line and column numbers.")]
    fn find_references(
        &self,
        Parameters(PositionParams {
            file_path,
            line,
            column,
        }): Parameters<PositionParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let line0 = line.saturating_sub(1);
        let col0 = column.saturating_sub(1);
        let position = Position::new(line0, col0);

        let refs = providers::references::find_references(
            &self.index,
            &file.source,
            &file.line_index,
            position,
            &uri,
            true,
        );

        if refs.is_empty() {
            return "No references found.".to_string();
        }

        let mut output = Vec::new();
        for loc in &refs {
            let ref_path = loc
                .uri
                .to_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| loc.uri.to_string());
            output.push(format!(
                "{}:{}:{}",
                ref_path,
                loc.range.start.line + 1,
                loc.range.start.character + 1,
            ));
        }

        format!("{} references:\n{}", refs.len(), output.join("\n"))
    }

    #[tool(description = "List all top-level symbols (functions, structs, constants, global variables) defined in a NWScript file. Returns name, kind, line number, and type signature for each.")]
    fn get_symbols(
        &self,
        Parameters(FileParams { file_path }): Parameters<FileParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        if file.symbols.is_empty() {
            return "No symbols found in file.".to_string();
        }

        let mut output = Vec::new();
        for sym in &file.symbols {
            if sym.kind == SymbolKind::StructField {
                continue;
            }
            let kind = match sym.kind {
                SymbolKind::Function => "fn",
                SymbolKind::Struct => "struct",
                SymbolKind::Constant => "const",
                SymbolKind::Variable => "var",
                SymbolKind::StructField => "field",
            };
            let (line, _) = file.line_index.line_col(sym.span.start);
            output.push(format!("  {:>6} | L{:<4} | {}", kind, line + 1, sym.detail));
        }

        format!(
            "{} symbols in {}:\n{}",
            output.len(),
            path.display(),
            output.join("\n"),
        )
    }

    #[tool(description = "Format a NWScript (.nss) file using the project's formatter. Returns the formatted source code. Does not modify the file on disk.")]
    fn format_file(
        &self,
        Parameters(FileParams { file_path }): Parameters<FileParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(source) = read_file_lossy(&path) else {
            return format!("Error: failed to read {}", path.display());
        };

        let config = nwscript_parser::FormatConfig::default();
        let formatted = nwscript_parser::formatter::format(&source, &config);

        if formatted == source {
            "File is already formatted -- no changes needed.".to_string()
        } else {
            formatted
        }
    }

    #[tool(description = "Search for a NWScript symbol by name across the entire workspace. Returns matching symbols with their type, file location, and documentation. If file_path is provided, searches symbols visible from that file's include tree first.")]
    fn find_symbol(
        &self,
        Parameters(SymbolSearchParams { name, file_path }): Parameters<SymbolSearchParams>,
    ) -> String {
        // If a file context is given, search visible symbols first.
        if let Some(fp) = &file_path {
            let path = resolve_path(fp);
            if let Some(uri) = self.refresh_file(&path) {
                if let Some(sym) = self.index.find_symbol(&uri, &name) {
                    return format!("Found (visible from {}):\n{}", fp, format_symbol(&sym));
                }
            }
        }

        // Search all workspace symbols.
        let all = self.index.all_workspace_symbols();
        let matches: Vec<&SymbolInfo> = all.iter().filter(|s| s.name == name).collect();

        if matches.is_empty() {
            // Try substring match
            let partial: Vec<&SymbolInfo> = all
                .iter()
                .filter(|s| s.name.to_lowercase().contains(&name.to_lowercase()))
                .take(20)
                .collect();

            if partial.is_empty() {
                return format!("No symbol named '{}' found in workspace.", name);
            }

            let mut output = format!(
                "No exact match for '{}'. {} partial matches:\n",
                name,
                partial.len(),
            );
            for sym in &partial {
                output.push_str(&format_symbol(sym));
                output.push('\n');
            }
            return output;
        }

        let mut output = format!("{} match(es) for '{}':\n", matches.len(), name);
        for sym in &matches {
            output.push_str(&format_symbol(sym));
            output.push('\n');
        }
        output
    }

    #[tool(description = "Rename a NWScript symbol across the entire workspace. Finds all references and applies the rename to files on disk. Returns a summary of all files modified. Uses 1-based line and column numbers.")]
    fn rename_symbol(
        &self,
        Parameters(RenameParams {
            file_path,
            line,
            column,
            new_name,
        }): Parameters<RenameParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let line0 = line.saturating_sub(1);
        let col0 = column.saturating_sub(1);
        let position = Position::new(line0, col0);

        // Validate the identifier at the position
        let Some(offset) = file.line_index.offset(line0, col0) else {
            return "Error: position out of range".to_string();
        };
        let Some(old_name) = providers::hover::find_ident_at(&file.source, offset as usize) else {
            return "No identifier at position".to_string();
        };

        // Validate new name is a legal identifier
        if new_name.is_empty()
            || !new_name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || new_name.as_bytes()[0].is_ascii_digit()
        {
            return format!("Error: '{}' is not a valid NWScript identifier", new_name);
        }

        // Find all references (includes the declaration)
        let refs = providers::references::find_references(
            &self.index,
            &file.source,
            &file.line_index,
            position,
            &uri,
            true,
        );

        if refs.is_empty() {
            return format!("No references found for '{}'.", old_name);
        }

        // Group edits by file URI
        let mut edits_by_file: HashMap<Url, Vec<(u32, u32, u32, u32)>> = HashMap::new();
        for loc in &refs {
            edits_by_file.entry(loc.uri.clone()).or_default().push((
                loc.range.start.line,
                loc.range.start.character,
                loc.range.end.line,
                loc.range.end.character,
            ));
        }

        // Apply edits to each file (process in reverse order to preserve offsets)
        let mut modified_files = Vec::new();
        let mut total_replacements = 0;

        for (file_uri, ranges) in &edits_by_file {
            let Some(indexed) = self.index.get_file(file_uri) else {
                continue;
            };
            let file_path = match file_uri.to_file_path() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let mut source = indexed.source.clone();

            // Sort ranges in reverse order so replacements don't shift later offsets
            let mut sorted_ranges = ranges.clone();
            sorted_ranges.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

            let mut count = 0;
            for (sl, sc, el, ec) in &sorted_ranges {
                let Some(start_off) = indexed.line_index.offset(*sl, *sc) else {
                    continue;
                };
                let Some(end_off) = indexed.line_index.offset(*el, *ec) else {
                    continue;
                };
                let start = start_off as usize;
                let end = end_off as usize;
                if start <= end && end <= source.len() {
                    source.replace_range(start..end, &new_name);
                    count += 1;
                }
            }

            if count > 0 {
                if let Err(e) = std::fs::write(&file_path, &source) {
                    return format!(
                        "Error: failed to write {}: {}",
                        file_path.display(),
                        e
                    );
                }
                // Re-index the modified file
                self.index
                    .update_file(file_uri, source);
                modified_files.push(format!(
                    "  {} ({} replacement{})",
                    file_path.display(),
                    count,
                    if count == 1 { "" } else { "s" },
                ));
                total_replacements += count;
            }
        }

        format!(
            "Renamed '{}' -> '{}': {} replacement{} across {} file{}:\n{}",
            old_name,
            new_name,
            total_replacements,
            if total_replacements == 1 { "" } else { "s" },
            modified_files.len(),
            if modified_files.len() == 1 { "" } else { "s" },
            modified_files.join("\n"),
        )
    }

    #[tool(description = "Run diagnostics across all NWScript (.nss) files in the workspace. Returns parse errors and optionally unused import hints for every file that has issues. Useful for finding all problems at once.")]
    fn workspace_diagnostics(
        &self,
        Parameters(WorkspaceDiagnosticsParams { include_hints }): Parameters<
            WorkspaceDiagnosticsParams,
        >,
    ) -> String {
        let include_hints = include_hints.unwrap_or(false);
        let all_uris = self.index.all_files();

        let mut files_with_errors = 0;
        let mut total_errors = 0;
        let mut total_hints = 0;
        let mut output = Vec::new();

        for file_uri in &all_uris {
            let Some(file) = self.index.get_file(file_uri) else {
                continue;
            };

            let file_path = file_uri
                .to_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| file_uri.to_string());

            let mut file_diags = Vec::new();

            // Parser errors
            for err in &file.parsed.errors {
                let (line, col) = file.line_index.line_col(err.span.start);
                file_diags.push(format!(
                    "  {}:{}: error: {}",
                    line + 1,
                    col + 1,
                    err.message,
                ));
                total_errors += 1;
            }

            // Unused import hints
            if include_hints {
                let analysis = providers::actions::analyze_imports(
                    &self.index,
                    file_uri,
                    &file.parsed,
                    &file.source,
                    &file.line_index,
                );
                for diag in &analysis.diagnostics {
                    file_diags.push(format!(
                        "  {}:{}: hint: {}",
                        diag.range.start.line + 1,
                        diag.range.start.character + 1,
                        diag.message,
                    ));
                    total_hints += 1;
                }
            }

            if !file_diags.is_empty() {
                files_with_errors += 1;
                output.push(format!("{}:\n{}", file_path, file_diags.join("\n")));
            }
        }

        if output.is_empty() {
            format!(
                "No diagnostics found across {} files.",
                all_uris.len(),
            )
        } else {
            let mut summary = format!(
                "{} error{} in {} file{} (out of {} total)",
                total_errors,
                if total_errors == 1 { "" } else { "s" },
                files_with_errors,
                if files_with_errors == 1 { "" } else { "s" },
                all_uris.len(),
            );
            if include_hints {
                summary.push_str(&format!(
                    ", {} unused import hint{}",
                    total_hints,
                    if total_hints == 1 { "" } else { "s" },
                ));
            }
            summary.push_str(":\n\n");
            summary.push_str(&output.join("\n\n"));
            summary
        }
    }

    #[tool(description = "Show the #include dependency tree for a NWScript file -- what files it includes (directly and transitively) and total visible symbol count.")]
    fn get_includes(
        &self,
        Parameters(FileParams { file_path }): Parameters<FileParams>,
    ) -> String {
        let path = resolve_path(&file_path);
        let Some(uri) = self.refresh_file(&path) else {
            return format!("Error: failed to read {}", path.display());
        };
        let Some(file) = self.index.get_file(&uri) else {
            return "Error: file not indexed".to_string();
        };

        let mut output = format!("Include tree for {}:\n", path.display());

        if file.includes.is_empty() {
            output.push_str("  No #include directives.\n");
        } else {
            output.push_str("  Direct includes:\n");
            for inc in &file.includes {
                let resolved = self
                    .index
                    .resolve_include(inc)
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(not found)".to_string());
                output.push_str(&format!("    #include \"{}\" -> {}\n", inc, resolved));
            }
        }

        let visible = self.index.visible_symbols(&uri);
        output.push_str(&format!(
            "\n  Total visible symbols (including transitive): {}",
            visible.len(),
        ));

        output
    }
}

// =============================================================================
// Entry point
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // Determine workspace directory: first CLI arg, or current directory.
    let workspace_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine working directory"));

    tracing::info!(
        "initializing NWScript MCP server for {}",
        workspace_dir.display()
    );

    let server = NwscriptMcp::new(&workspace_dir);

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
