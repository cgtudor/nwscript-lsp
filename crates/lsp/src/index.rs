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
    /// Broader search roots (workspace dirs + source dir parents) for special files.
    search_roots: Vec<PathBuf>,
    /// Directory names to skip when scanning (dot-prefixed dirs are always skipped).
    exclude_dirs: Vec<String>,
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
    /// True if this is a function definition (has body), false for prototypes.
    pub is_definition: bool,
    /// The include name for auto-import (file stem without extension).
    pub include_name: String,
    /// For constants/variables: raw text of the initializer expression.
    pub initializer_text: Option<String>,
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
    pub fn new(
        source_dirs: Vec<PathBuf>,
        workspace_dirs: Vec<PathBuf>,
        exclude_dirs: Vec<String>,
    ) -> Self {
        // Collect all root dirs to search for special files like nwscript.nss.
        // Includes workspace roots AND parents of source dirs.
        let mut search_roots = workspace_dirs;
        for src in &source_dirs {
            if let Some(parent) = src.parent() {
                let p = parent.to_path_buf();
                if !search_roots.contains(&p) {
                    search_roots.push(p);
                }
            }
        }

        Self {
            files: DashMap::new(),
            include_map: DashMap::new(),
            source_dirs,
            search_roots,
            exclude_dirs,
        }
    }

    /// Scan all source directories and index every .nss file found.
    ///
    /// If `nwscript_nss_path` is provided, it is used directly instead of
    /// searching for `nwscript.nss` recursively.
    ///
    /// If `vanilla_cache_dir` is provided, vanilla scripts from KEY/BIF are
    /// indexed first at lowest priority — workspace files override them.
    pub fn scan_workspace(
        &self,
        nwscript_nss_path: Option<&Path>,
        vanilla_cache_dir: Option<&Path>,
    ) {
        // Phase 1: Index vanilla scripts from KEY/BIF cache (lowest priority).
        // These get overridden by workspace files indexed in phase 2.
        if let Some(cache_dir) = vanilla_cache_dir {
            let mut vanilla_files = Vec::new();
            collect_nss_files(cache_dir, &[], &mut vanilla_files);
            tracing::info!("indexing {} vanilla .nss files from KEY/BIF", vanilla_files.len());
            for path in vanilla_files {
                if let Err(e) = self.index_file_from_disk(&path) {
                    tracing::warn!("failed to index vanilla {}: {e}", path.display());
                }
            }
        }

        // Phase 2: Index workspace files (override vanilla).
        let mut nss_files = Vec::new();

        for dir in &self.source_dirs {
            collect_nss_files(dir, &self.exclude_dirs, &mut nss_files);
        }

        // Find nwscript.nss (the engine built-in definitions).
        // If we already got it from vanilla cache, workspace copy still overrides.
        if let Some(explicit_path) = nwscript_nss_path {
            if explicit_path.exists() && !nss_files.contains(&explicit_path.to_path_buf()) {
                nss_files.push(explicit_path.to_path_buf());
            }
        } else if vanilla_cache_dir.is_none() {
            // Only auto-discover if we didn't get it from vanilla extraction
            for dir in &self.search_roots {
                find_file_recursive(dir, "nwscript.nss", &mut nss_files);
            }
        }

        tracing::info!("indexing {} workspace .nss files", nss_files.len());

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
        let source = read_file_lossy(path)?;
        let uri = Url::from_file_path(path).map_err(|_| "invalid path".to_string())?;
        self.index_file_with_source(uri, path.to_path_buf(), source);
        Ok(())
    }

    /// Index a file with known source text (used for open documents).
    pub fn index_file_with_source(&self, uri: Url, path: PathBuf, source: String) {
        let uri = normalize_uri(&uri);
        let line_index = LineIndex::new(&source);
        let parsed = nwscript_parser::parse(&source);

        let include_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let includes = extract_includes(&parsed);
        let symbols = extract_symbols(&parsed, &uri, &source, &include_name);

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

    /// Get ALL symbols from all indexed files (for auto-import completion).
    pub fn all_workspace_symbols(&self) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        for entry in self.files.iter() {
            symbols.extend(entry.value().symbols.iter().cloned());
        }
        symbols
    }

    /// Collect all include names in the transitive include tree of `uri`.
    /// Returns a set of lowercased include names that are reachable.
    pub fn include_tree_set(&self, uri: &Url) -> HashSet<String> {
        let uri = normalize_uri(uri);
        let mut visited = HashSet::new();
        let mut include_names = HashSet::new();
        self.collect_include_tree(&uri, &mut visited, &mut include_names);
        // nwscript.nss is always implicitly included
        include_names.insert("nwscript".to_string());
        include_names
    }

    fn collect_include_tree(
        &self,
        uri: &Url,
        visited: &mut HashSet<Url>,
        include_names: &mut HashSet<String>,
    ) {
        if !visited.insert(uri.clone()) {
            return;
        }
        let Some(file) = self.get_file(uri) else {
            return;
        };
        if let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) {
            include_names.insert(stem.to_lowercase());
        }
        for inc in &file.includes {
            include_names.insert(inc.to_lowercase());
            if let Some(inc_uri) = self.resolve_include(inc) {
                self.collect_include_tree(&inc_uri, visited, include_names);
            }
        }
    }

    /// Call a closure for each symbol across all indexed files, without cloning.
    pub fn for_each_symbol<F>(&self, mut f: F)
    where
        F: FnMut(&SymbolInfo),
    {
        for entry in self.files.iter() {
            for sym in &entry.value().symbols {
                f(sym);
            }
        }
    }

    /// Check if a file (by include name) is in the transitive include tree of `uri`.
    pub fn is_in_include_tree(&self, uri: &Url, include_name: &str) -> bool {
        let uri = normalize_uri(uri);
        let mut visited = HashSet::new();
        self.check_include_tree(&uri, include_name, &mut visited)
    }

    fn check_include_tree(&self, uri: &Url, target: &str, visited: &mut HashSet<Url>) -> bool {
        if !visited.insert(uri.clone()) {
            return false;
        }
        let Some(file) = self.get_file(uri) else {
            return false;
        };
        // Check if this file's stem matches
        if let Some(stem) = file.path.file_stem().and_then(|s| s.to_str()) {
            if stem.eq_ignore_ascii_case(target) {
                return true;
            }
        }
        // Check includes
        for inc in &file.includes {
            if inc.eq_ignore_ascii_case(target) {
                return true;
            }
            if let Some(inc_uri) = self.resolve_include(inc) {
                if self.check_include_tree(&inc_uri, target, visited) {
                    return true;
                }
            }
        }
        false
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

fn extract_symbols(parsed: &ParsedFile, uri: &Url, source: &str, include_name: &str) -> Vec<SymbolInfo> {
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
                        is_definition: !f.is_prototype(),
                        include_name: include_name.to_string(),
                        initializer_text: None,
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
                        is_definition: true,
                        include_name: include_name.to_string(),
                        initializer_text: None,
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
                                is_definition: true,
                                include_name: include_name.to_string(),
                                initializer_text: None,
                            });
                        }
                    }
                }
            }
            Declaration::GlobalVar(v) => {
                if let Some(name) = &v.name {
                    let ty = crate::providers::symbols::format_type(&v.ty.kind);
                    let init_text = v
                        .initializer
                        .as_ref()
                        .map(|expr| expr.span().text(source).to_string());
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
                        is_definition: true,
                        include_name: include_name.to_string(),
                        initializer_text: init_text,
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
/// Dot-prefixed directories are always skipped. Additional names come from config.
fn should_skip_dir(name: &str, exclude_dirs: &[String]) -> bool {
    name.starts_with('.') || exclude_dirs.iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// Read a file, trying UTF-8 first, falling back to Latin-1/Windows-1252.
/// NWScript files from the BioWare era often use Windows-1252 encoding.
fn read_file_lossy(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;

    // Try UTF-8 first
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Ok(s),
        Err(_) => {
            // Fall back to Latin-1 decoding (every byte is valid)
            Ok(bytes.iter().map(|&b| b as char).collect())
        }
    }
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

fn collect_nss_files(dir: &Path, exclude_dirs: &[String], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_dir(name, exclude_dirs) {
                continue;
            }
            collect_nss_files(&path, exclude_dirs, out);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("nss")) {
            out.push(path);
        }
    }
}
