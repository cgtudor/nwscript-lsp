use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use nwscript_parser::{Declaration, LineIndex, ParsedFile};
use tower_lsp::lsp_types::Url;

/// Normalize a URI for consistent lookups.
/// On Windows, VS Code sends `file:///c:/...` (lowercase drive) while
/// `Url::from_file_path` produces `file:///C:/...` (uppercase). Roundtrip
/// through file path to canonicalize.
pub fn normalize_uri(uri: &Url) -> Url {
    if let Ok(path) = uri.to_file_path() {
        Url::from_file_path(&path).unwrap_or_else(|_| uri.clone())
    } else {
        uri.clone()
    }
}

/// Workspace-wide index of all NWScript files and their symbols.
pub struct WorkspaceIndex {
    /// All indexed files keyed by normalized URI.
    files: DashMap<Url, Arc<IndexedFile>>,
    /// Map from include name (e.g., "nwnx_player") to normalized file URI.
    include_map: DashMap<String, Url>,
    /// Source directories to search for files.
    source_dirs: Vec<PathBuf>,
}

/// A parsed and indexed .nss file.
pub struct IndexedFile {
    pub uri: Url,
    pub path: PathBuf,
    pub source: String,
    pub line_index: LineIndex,
    pub parsed: ParsedFile,
    /// Include names from #include directives (without extension).
    pub includes: Vec<String>,
    /// Symbols defined in this file (functions, structs, constants, variables).
    pub symbols: Vec<SymbolInfo>,
}

/// A symbol extracted from a declaration, usable for cross-file lookups.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: Url,
    pub span: nwscript_parser::Span,
    /// Full declaration span (for document symbols).
    pub decl_span: nwscript_parser::Span,
    /// Type signature for display.
    pub detail: String,
    /// Documentation comment, if any.
    pub doc: Option<String>,
    /// For functions: parameter info.
    pub params: Option<Vec<ParamInfo>>,
    /// For functions: return type display string.
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub ty: String,
    pub has_default: bool,
    pub default_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Constant,
    Variable,
    StructField,
}

impl WorkspaceIndex {
    pub fn new(source_dirs: Vec<PathBuf>) -> Self {
        Self {
            files: DashMap::new(),
            include_map: DashMap::new(),
            source_dirs,
        }
    }

    /// Scan all source directories and index every .nss file found.
    pub fn scan_workspace(&self) {
        let mut nss_files = Vec::new();

        for dir in &self.source_dirs {
            collect_nss_files(dir, &mut nss_files);
        }

        // Also find nwscript.nss specifically (the engine built-in definitions).
        // It may live in a docs/ or reference directory that we normally skip.
        for dir in &self.source_dirs {
            find_file_recursive(dir, "nwscript.nss", &mut nss_files);
        }

        tracing::info!("indexing {} .nss files", nss_files.len());

        for path in nss_files {
            if let Err(e) = self.index_file_from_disk(&path) {
                tracing::warn!("failed to index {}: {e}", path.display());
            }
        }

        tracing::info!(
            "workspace index complete: {} files, {} include mappings",
            self.files.len(),
            self.include_map.len()
        );
    }

    /// Index a single file from disk.
    fn index_file_from_disk(&self, path: &Path) -> Result<(), String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let uri = Url::from_file_path(path).map_err(|_| "invalid path".to_string())?;
        self.index_file_with_source(uri, path.to_path_buf(), source);
        Ok(())
    }

    /// Index a file with known source text (used for open documents).
    pub fn index_file_with_source(&self, uri: Url, path: PathBuf, source: String) {
        let uri = normalize_uri(&uri);
        let line_index = LineIndex::new(&source);
        let parsed = nwscript_parser::parse(&source);

        let includes = extract_includes(&parsed);
        let symbols = extract_symbols(&parsed, &uri, &source);

        // Register include mapping: stem name -> URI
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            self.include_map
                .insert(stem.to_lowercase(), uri.clone());
        }

        let indexed = Arc::new(IndexedFile {
            uri: uri.clone(),
            path,
            source,
            line_index,
            parsed,
            includes,
            symbols,
        });

        self.files.insert(uri, indexed);
    }

    /// Update a file in the index (when it's edited in the editor).
    pub fn update_file(&self, uri: &Url, source: String) {
        let path = uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()));
        self.index_file_with_source(uri.clone(), path, source);
    }

    /// Get an indexed file by URI.
    pub fn get_file(&self, uri: &Url) -> Option<Arc<IndexedFile>> {
        let uri = normalize_uri(uri);
        self.files.get(&uri).map(|f| Arc::clone(f.value()))
    }

    /// Resolve an include name to a URI.
    pub fn resolve_include(&self, name: &str) -> Option<Url> {
        self.include_map
            .get(&name.to_lowercase())
            .map(|r| r.value().clone())
    }

    /// Get all symbols visible from a file (own symbols + transitive includes + implicit nwscript.nss).
    pub fn visible_symbols(&self, uri: &Url) -> Vec<SymbolInfo> {
        let uri = normalize_uri(uri);
        let mut visited = HashSet::new();
        let mut symbols = Vec::new();
        self.collect_symbols_recursive(&uri, &mut visited, &mut symbols);

        // nwscript.nss is implicitly included by every file (engine built-in).
        if let Some(nwscript_uri) = self.resolve_include("nwscript") {
            self.collect_symbols_recursive(&nwscript_uri, &mut visited, &mut symbols);
        }

        symbols
    }

    fn collect_symbols_recursive(
        &self,
        uri: &Url,
        visited: &mut HashSet<Url>,
        symbols: &mut Vec<SymbolInfo>,
    ) {
        if !visited.insert(uri.clone()) {
            return; // Cycle detection
        }

        let Some(file) = self.get_file(uri) else {
            return;
        };

        // Add this file's symbols
        symbols.extend(file.symbols.iter().cloned());

        // Recurse into includes
        for inc_name in &file.includes {
            if let Some(inc_uri) = self.resolve_include(inc_name) {
                self.collect_symbols_recursive(&inc_uri, visited, symbols);
            }
        }
    }

    /// Find a symbol by name visible from a given file.
    pub fn find_symbol(&self, uri: &Url, name: &str) -> Option<SymbolInfo> {
        let symbols = self.visible_symbols(uri);
        // Prefer definitions (non-prototypes) over prototypes
        let mut best: Option<SymbolInfo> = None;
        for sym in symbols {
            if sym.name == name {
                if best.is_none() || sym.kind == SymbolKind::Function {
                    best = Some(sym);
                }
            }
        }
        best
    }

    /// Get all files in the index.
    pub fn all_files(&self) -> Vec<Url> {
        self.files.iter().map(|r| r.key().clone()).collect()
    }
}

// =============================================================================
// Symbol extraction
// =============================================================================

fn extract_includes(parsed: &ParsedFile) -> Vec<String> {
    parsed
        .declarations
        .iter()
        .filter_map(|decl| match decl {
            Declaration::Include(inc) => inc.path.clone(),
            _ => None,
        })
        .collect()
}

fn extract_symbols(parsed: &ParsedFile, uri: &Url, source: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            Declaration::Function(f) => {
                if let Some(name) = &f.name {
                    let params: Vec<ParamInfo> = f
                        .params
                        .iter()
                        .map(|p| {
                            let ty = crate::providers::symbols::format_type(&p.ty.kind);
                            let pname = p
                                .name
                                .as_ref()
                                .map(|n| n.name.clone())
                                .unwrap_or_default();
                            let default_text = p.default_value.as_ref().map(|expr| {
                                expr.span().text(source).to_string()
                            });
                            ParamInfo {
                                name: pname,
                                ty,
                                has_default: p.default_value.is_some(),
                                default_text,
                            }
                        })
                        .collect();

                    let ret_ty = crate::providers::symbols::format_type(&f.return_type.kind);
                    let detail = crate::providers::symbols::format_function_signature(f);
                    let doc = find_leading_comment(source, f.span);

                    symbols.push(SymbolInfo {
                        name: name.name.clone(),
                        kind: SymbolKind::Function,
                        uri: uri.clone(),
                        span: name.span,
                        decl_span: f.span,
                        detail,
                        doc,
                        params: Some(params),
                        return_type: Some(ret_ty),
                    });
                }
            }
            Declaration::Struct(s) => {
                if let Some(name) = &s.name {
                    let fields_desc: Vec<String> = s
                        .fields
                        .iter()
                        .map(|f| {
                            let ty = crate::providers::symbols::format_type(&f.ty.kind);
                            let fname = f
                                .name
                                .as_ref()
                                .map(|n| n.name.as_str())
                                .unwrap_or("?");
                            format!("{ty} {fname}")
                        })
                        .collect();

                    symbols.push(SymbolInfo {
                        name: name.name.clone(),
                        kind: SymbolKind::Struct,
                        uri: uri.clone(),
                        span: name.span,
                        decl_span: s.span,
                        detail: format!("struct {{ {} }}", fields_desc.join("; ")),
                        doc: find_leading_comment(source, s.span),
                        params: None,
                        return_type: None,
                    });

                    // Also index struct fields for field access completion
                    for field in &s.fields {
                        if let Some(fname) = &field.name {
                            symbols.push(SymbolInfo {
                                name: fname.name.clone(),
                                kind: SymbolKind::StructField,
                                uri: uri.clone(),
                                span: fname.span,
                                decl_span: field.span,
                                detail: format!(
                                    "{}.{}: {}",
                                    name.name,
                                    fname.name,
                                    crate::providers::symbols::format_type(&field.ty.kind)
                                ),
                                doc: None,
                                params: None,
                                return_type: None,
                            });
                        }
                    }
                }
            }
            Declaration::GlobalVar(v) => {
                if let Some(name) = &v.name {
                    let ty = crate::providers::symbols::format_type(&v.ty.kind);
                    symbols.push(SymbolInfo {
                        name: name.name.clone(),
                        kind: if v.is_const {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        },
                        uri: uri.clone(),
                        span: name.span,
                        decl_span: v.span,
                        detail: if v.is_const {
                            format!("const {ty}")
                        } else {
                            ty
                        },
                        doc: find_leading_comment(source, v.span),
                        params: None,
                        return_type: None,
                    });
                }
            }
            Declaration::Include(_) => {}
        }
    }

    symbols
}

/// Extract leading `//` comments before a span as documentation.
fn find_leading_comment(source: &str, span: nwscript_parser::Span) -> Option<String> {
    let before = &source[..span.start as usize];
    let mut lines: Vec<&str> = Vec::new();

    for line in before.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            let doc = trimmed.trim_start_matches('/').trim();
            lines.push(doc);
        } else if trimmed.is_empty() {
            if !lines.is_empty() {
                break;
            }
        } else {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    Some(lines.join("\n"))
}

// =============================================================================
// File discovery
// =============================================================================

/// Directories to skip when scanning for .nss files.
fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.')
        || name == "docs"
        || name == "nwn_source"
        || name == "node_modules"
        || name == "target"
}

/// Search recursively for a specific file, ignoring skip rules.
fn find_file_recursive(dir: &Path, target: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_file_recursive(&path, target, out);
        } else if path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(target))
        {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
}

fn collect_nss_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_dir(name) {
                continue;
            }
            collect_nss_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("nss")) {
            out.push(path);
        }
    }
}
