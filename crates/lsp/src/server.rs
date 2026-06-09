use std::path::PathBuf;
use std::sync::RwLock;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::document::DocumentStore;
use crate::index::WorkspaceIndex;
use crate::providers;

/// Inlay hints settings from the client.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsSettings {
    pub enabled: Option<bool>,
    pub suppress_for_single_arg_calls: Option<bool>,
}

/// Configuration received from the client.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NwscriptConfig {
    pub compiler_path: Option<String>,
    pub include_dirs: Option<Vec<String>>,
    /// Directory names to exclude from source scanning
    /// (dot-prefixed directories are always excluded).
    pub exclude_dirs: Option<Vec<String>>,
    /// Explicit path to nwscript.nss (engine built-in definitions).
    /// If empty/None, the LSP searches workspace directories recursively.
    pub nwscript_nss_path: Option<String>,
    /// Path to NWN:EE installation directory (contains data/ with KEY/BIF files).
    /// If empty/None, auto-detected from NWN_ROOT env var, Steam, Beamdog, or GOG.
    pub nwn_root: Option<String>,
    /// Whether to extract vanilla .nss scripts from KEY/BIF files.
    /// Defaults to true. Set to false to skip extraction (only nwscript.nss will be used).
    pub extract_vanilla_scripts: Option<bool>,
    #[serde(default)]
    pub inlay_hints: Option<InlayHintsSettings>,
    #[serde(default)]
    pub formatter: providers::formatting::FormatterSettings,
}

pub struct NwscriptLanguageServer {
    client: Client,
    documents: DocumentStore,
    index: RwLock<Option<WorkspaceIndex>>,
    config: RwLock<NwscriptConfig>,
    /// Nasher cache directory (flat copy of all .nss files, used for compilation).
    nasher_cache: RwLock<Option<PathBuf>>,
    /// Fallback: all directories containing .nss files.
    nss_dirs: RwLock<Vec<PathBuf>>,
    /// NWN:EE installation root (for --root compiler flag).
    nwn_root: RwLock<Option<PathBuf>>,
    /// NWN:EE user directory (for --userdirectory compiler flag).
    nwn_home: RwLock<Option<PathBuf>>,
    /// Last compiler diagnostics per file (cleared on edit, updated on save).
    compiler_diags: DashMap<Url, Vec<Diagnostic>>,
}

impl NwscriptLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
            index: RwLock::new(None),
            config: RwLock::new(NwscriptConfig::default()),
            nasher_cache: RwLock::new(None),
            nss_dirs: RwLock::new(Vec::new()),
            nwn_root: RwLock::new(None),
            nwn_home: RwLock::new(None),
            compiler_diags: DashMap::new(),
        }
    }

    /// Initialize the workspace index by scanning all source directories.
    fn initialize_index(&self, workspace_dirs: &[PathBuf]) {
        let mut source_dirs = Vec::new();

        // Discover source dirs from nasher.cfg files
        for ws_dir in workspace_dirs {
            let mut dirs = crate::nasher::discover_source_dirs(ws_dir);
            source_dirs.append(&mut dirs);
        }

        // Read config settings
        let (exclude_dirs, nwscript_nss_path, nwn_root_setting, extract_vanilla) = {
            let config = self.config.read().unwrap();

            // Also add any extra include dirs from config
            if let Some(extra) = &config.include_dirs {
                for dir in extra {
                    let p = PathBuf::from(dir);
                    if p.is_dir() && !source_dirs.contains(&p) {
                        source_dirs.push(p);
                    }
                }
            }

            let exclude = config
                .exclude_dirs
                .clone()
                .unwrap_or_else(default_exclude_dirs);

            let nss_path = config
                .nwscript_nss_path
                .as_ref()
                .filter(|p| !p.is_empty())
                .map(PathBuf::from);

            let nwn_root = config
                .nwn_root
                .as_ref()
                .filter(|p| !p.is_empty())
                .map(|s| s.as_str())
                .map(String::from);

            let extract_vanilla = config.extract_vanilla_scripts.unwrap_or(true);

            (exclude, nss_path, nwn_root, extract_vanilla)
        };

        tracing::info!("exclude dirs: {:?}", exclude_dirs);

        // Extract vanilla .nss scripts from NWN:EE installation KEY/BIF files.
        // These are written to a cache directory and indexed at lowest priority
        // (workspace files override them). Can be disabled via extractVanillaScripts setting.
        let (vanilla_cache_dir, nwn_root) = if extract_vanilla {
            extract_vanilla_scripts(nwn_root_setting.as_deref())
        } else {
            tracing::info!("vanilla script extraction disabled by setting");
            // Still detect NWN root for compiler --root flag
            let nwn_root = crate::nwn_install::find_nwn_root(nwn_root_setting.as_deref());
            (None, nwn_root)
        };
        *self.nwn_root.write().unwrap() = nwn_root;

        // Detect NWN user directory for compiler --userdirectory flag
        *self.nwn_home.write().unwrap() = find_nwn_home();

        // Find nasher cache for compiler diagnostics (preferred over --dirs).
        // Search both workspace dirs and parents of source dirs (nasher.cfg is
        // typically next to the .nasher/ directory).
        let mut cache_search_dirs = workspace_dirs.to_vec();
        for src_dir in &source_dirs {
            // Walk up from src dir to find .nasher/
            let mut dir = src_dir.as_path();
            while let Some(parent) = dir.parent() {
                if !cache_search_dirs.contains(&parent.to_path_buf()) {
                    cache_search_dirs.push(parent.to_path_buf());
                }
                dir = parent;
                // Don't go further than 3 levels up
                if cache_search_dirs.len() > 20 {
                    break;
                }
            }
        }
        let nasher_cache = crate::diagnostics::find_nasher_cache(&cache_search_dirs);
        match &nasher_cache {
            Some(cache) => tracing::info!("using nasher cache for compiler: {}", cache.display()),
            None => tracing::warn!("no nasher cache found — compiler diagnostics may show false positives"),
        }
        *self.nasher_cache.write().unwrap() = nasher_cache;

        // Fallback: collect all directories containing .nss files
        let nss_dirs =
            crate::diagnostics::collect_nss_directories(&source_dirs, &exclude_dirs);
        tracing::info!("found {} directories containing .nss files", nss_dirs.len());
        *self.nss_dirs.write().unwrap() = nss_dirs;

        let index =
            WorkspaceIndex::new(source_dirs, workspace_dirs.to_vec(), exclude_dirs);
        index.scan_workspace(nwscript_nss_path.as_deref(), vanilla_cache_dir.as_deref());

        *self.index.write().unwrap() = Some(index);
    }

    /// Get a reference to the workspace index.
    fn with_index<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&WorkspaceIndex) -> R,
    {
        let guard = self.index.read().unwrap();
        guard.as_ref().map(f)
    }

    /// Publish combined parser + compiler diagnostics for a document.
    /// Called on every edit (didChange). Clears stale compiler diagnostics.
    async fn publish_diagnostics_for(&self, uri: &Url, clear_compiler: bool) {
        let Some(doc) = self.documents.get(uri) else {
            return;
        };

        // Update workspace index
        {
            let guard = self.index.read().unwrap();
            if let Some(index) = guard.as_ref() {
                index.update_file(uri, doc.source.clone());
            }
        }

        // Parser diagnostics (always fresh from current source)
        let mut diagnostics: Vec<Diagnostic> = doc
            .parsed
            .errors
            .iter()
            .map(|err| {
                let (sl, sc) = doc.line_index.line_col(err.span.start);
                let (el, ec) = doc.line_index.line_col(err.span.end);
                Diagnostic {
                    range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("nwscript-parser".into()),
                    message: err.message.clone(),
                    ..Default::default()
                }
            })
            .collect();

        // Clear stale compiler diagnostics when the user edits
        if clear_compiler {
            self.compiler_diags.remove(uri);
        }

        // Merge in any cached compiler diagnostics
        if let Some(compiler) = self.compiler_diags.get(uri) {
            diagnostics.extend(compiler.value().iter().cloned());
        }

        // Unused import diagnostics (grayed out with Unnecessary tag)
        {
            let guard = self.index.read().unwrap();
            if let Some(index) = guard.as_ref() {
                let analysis = providers::actions::analyze_imports(
                    index, uri, &doc.parsed, &doc.source, &doc.line_index,
                );
                diagnostics.extend(analysis.diagnostics);
            }
        }

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, Some(doc.version))
            .await;
    }

    /// Run the external compiler and merge results with parser diagnostics.
    async fn run_compiler_diagnostics(&self, uri: &Url) {
        let compiler_path = {
            let config = self.config.read().unwrap();
            match &config.compiler_path {
                Some(p) if !p.is_empty() => PathBuf::from(p),
                _ => {
                    find_bundled_compiler().unwrap_or_else(|| {
                        if cfg!(windows) {
                            PathBuf::from("nwn_script_comp.exe")
                        } else {
                            PathBuf::from("nwn_script_comp")
                        }
                    })
                }
            }
        };

        let file_path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        let source = match self.documents.get(uri) {
            Some(doc) => doc.source.clone(),
            None => match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(_) => return,
            },
        };

        let nasher_cache = self.nasher_cache.read().unwrap().clone();
        let fallback_dirs = self.nss_dirs.read().unwrap().clone();
        let nwn_root = self.nwn_root.read().unwrap().clone();
        let nwn_home = self.nwn_home.read().unwrap().clone();

        let compiler_diagnostics = crate::diagnostics::compile_file(
            &compiler_path,
            &file_path,
            &source,
            &nasher_cache,
            &fallback_dirs,
            &nwn_root,
            &nwn_home,
        )
        .await;

        // Store compiler diagnostics and publish combined set
        self.compiler_diags
            .insert(uri.clone(), compiler_diagnostics);
        self.publish_diagnostics_for(uri, false).await;
    }

    /// Get the source text at a given line up to the cursor position.
    fn get_text_before_cursor(&self, uri: &Url, position: Position) -> Option<String> {
        let doc = self.documents.get(uri)?;
        let offset = doc.line_index.offset(position.line, position.character)? as usize;
        // Get text from the start of the expression context (walk back to find statement start)
        // For simplicity, take the whole file up to cursor and let the signature parser handle it
        let text = &doc.source[..offset.min(doc.source.len())];

        // Take at most the last 500 chars to avoid scanning huge buffers
        let start = text.len().saturating_sub(500);
        Some(text[start..].to_string())
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for NwscriptLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Extract workspace folders for indexing
        let workspace_dirs: Vec<PathBuf> = params
            .workspace_folders
            .as_ref()
            .map(|folders| {
                folders
                    .iter()
                    .filter_map(|f| f.uri.to_file_path().ok())
                    .collect()
            })
            .or_else(|| {
                params
                    .root_uri
                    .as_ref()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| vec![p])
            })
            .unwrap_or_default();

        // Store config from initialization options
        if let Some(opts) = params.initialization_options {
            if let Ok(cfg) = serde_json::from_value::<NwscriptConfig>(opts) {
                *self.config.write().unwrap() = cfg;
            }
        }

        // Index workspace in background
        if !workspace_dirs.is_empty() {
            self.initialize_index(&workspace_dirs);
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "nwscript-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: providers::semantic_tokens::legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "}".into(),
                    more_trigger_character: Some(vec![";".into(), "\n".into()]),
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        let file_count = self
            .with_index(|idx| idx.all_files().len())
            .unwrap_or(0);
        self.client
            .log_message(
                MessageType::INFO,
                format!("nwscript-lsp initialized, {file_count} files indexed"),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let text = params.text_document.text;

        self.documents.open(uri.clone(), version, text);
        self.publish_diagnostics_for(&uri, false).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.update(&uri, version, change.text);
            // Clear compiler diagnostics on edit — they'll refresh on next save
            self.publish_diagnostics_for(&uri, true).await;

            // Also refresh diagnostics for other open documents so cross-file
            // changes (e.g., from rename) are reflected immediately.
            for other_uri in self.documents.all_uris() {
                if other_uri != uri {
                    self.publish_diagnostics_for(&other_uri, false).await;
                }
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.run_compiler_diagnostics(&params.text_document.uri)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.close(&uri);
        self.compiler_diags.remove(&uri);
        self.client
            .publish_diagnostics(uri, vec![], None)
            .await;
    }

    async fn did_change_watched_files(&self, _params: DidChangeWatchedFilesParams) {
        // TODO: re-index changed files
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(Some(CompletionResponse::Array(
                providers::completion::keyword_completions(),
            )));
        };

        let cursor_offset = doc
            .line_index
            .offset(position.line, position.character)
            .unwrap_or(0);

        let guard = self.index.read().unwrap();
        let items = match guard.as_ref() {
            Some(index) => {
                let mut items = providers::completion::completions_from_index(
                    index, uri, &doc.parsed, &doc.line_index, cursor_offset,
                );
                items.extend(providers::completion::keyword_completions());
                items
            }
            None => providers::completion::keyword_completions(),
        };

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let offset = match doc.line_index.offset(position.line, position.character) {
            Some(o) => o,
            None => return Ok(None),
        };

        let target_name = match providers::hover::find_ident_at(&doc.source, offset as usize) {
            Some(n) => n,
            None => return Ok(None),
        };

        // Check local variables/parameters first (higher priority)
        if let Some(detail) = providers::completion::find_local_detail(
            &doc.parsed,
            offset,
            &target_name,
        ) {
            return Ok(Some(providers::hover::hover_for_local(&detail)));
        }

        // Search cross-file symbols
        let hover = self.with_index(|index| {
            let symbols = index.visible_symbols(uri);
            symbols
                .iter()
                .find(|s| s.name == target_name)
                .map(providers::hover::hover_for_symbol)
        });

        Ok(hover.flatten())
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let location = self.with_index(|index| {
            providers::definition::goto_definition(
                index,
                &doc.source,
                &doc.line_index,
                position,
                uri,
            )
        });

        Ok(location.flatten().map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let refs = self.with_index(|index| {
            providers::references::find_references(
                index,
                &doc.source,
                &doc.line_index,
                position,
                uri,
                include_declaration,
            )
        });

        Ok(refs.filter(|r| !r.is_empty()))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let position = params.position;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let offset = match doc.line_index.offset(position.line, position.character) {
            Some(o) => o,
            None => return Ok(None),
        };

        let Some(name) =
            providers::hover::find_ident_at(&doc.source, offset as usize)
        else {
            return Ok(None);
        };

        // Verify it's a renamable symbol: either in the workspace index
        // (functions, globals, constants) or a local variable/parameter.
        let is_workspace_symbol = self
            .with_index(|index| {
                let symbols = index.visible_symbols(uri);
                symbols.iter().any(|s| s.name == name)
            })
            .unwrap_or(false);

        let is_local = providers::completion::is_local_symbol(
            &doc.parsed,
            offset,
            &name,
        );

        if !is_workspace_symbol && !is_local {
            return Ok(None);
        }

        // Return the range of the identifier under cursor
        let bytes = doc.source.as_bytes();
        let off = offset as usize;
        let mut start = off;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        let mut end = off;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }

        let (sl, sc) = doc.line_index.line_col(start as u32);
        let (el, ec) = doc.line_index.line_col(end as u32);
        let range = Range::new(Position::new(sl, sc), Position::new(el, ec));

        Ok(Some(PrepareRenameResponse::Range(range)))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = &params.new_name;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        // Find all references to this symbol
        let refs = self.with_index(|index| {
            providers::references::find_references(
                index,
                &doc.source,
                &doc.line_index,
                position,
                uri,
                true, // include declaration
            )
        });

        let Some(refs) = refs else {
            return Ok(None);
        };

        if refs.is_empty() {
            return Ok(None);
        }

        // Group edits by file URI
        let mut changes: std::collections::HashMap<Url, Vec<TextEdit>> =
            std::collections::HashMap::new();

        for loc in refs {
            changes
                .entry(loc.uri)
                .or_default()
                .push(TextEdit {
                    range: loc.range,
                    new_text: new_name.clone(),
                });
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let symbols = providers::symbols::document_symbols(&doc.parsed, &doc.line_index);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let text = match self.get_text_before_cursor(uri, position) {
            Some(t) => t,
            None => return Ok(None),
        };

        let help = self.with_index(|index| {
            let symbols = index.visible_symbols(uri);
            providers::signature::signature_help(&text, &symbols)
        });

        Ok(help.flatten())
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let config = {
            let cfg = self.config.read().unwrap();
            providers::formatting::build_config(&params.options, &cfg.formatter)
        };

        let edits = providers::formatting::format_document(&doc.source, &config);
        Ok(Some(edits))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        // For range formatting, we format the whole document.
        // This is standard practice (rustfmt, clang-format, etc.).
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let config = {
            let cfg = self.config.read().unwrap();
            providers::formatting::build_config(&params.options, &cfg.formatter)
        };

        let edits = providers::formatting::format_document(&doc.source, &config);
        Ok(Some(edits))
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document_position.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let config = {
            let cfg = self.config.read().unwrap();
            providers::formatting::build_config(&params.options, &cfg.formatter)
        };

        let edits = providers::formatting::on_type_format(
            &doc.source,
            params.text_document_position.position,
            &params.ch,
            &config,
        );
        Ok(Some(edits))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let guard = self.index.read().unwrap();
        let actions = match guard.as_ref() {
            Some(index) => {
                let analysis = providers::actions::analyze_imports(
                    index, uri, &doc.parsed, &doc.source, &doc.line_index,
                );
                analysis
                    .actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect()
            }
            None => Vec::new(),
        };

        Ok(Some(actions))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let result = self.with_index(|index| {
            providers::semantic_tokens::semantic_tokens(
                &doc.parsed,
                &doc.source,
                &doc.line_index,
                index,
                uri,
            )
        });

        Ok(result)
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let ranges =
            providers::folding::folding_ranges(&doc.parsed, &doc.source, &doc.line_index);

        Ok(Some(ranges))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let results = self.with_index(|index| {
            providers::workspace_symbols::workspace_symbols(index, &params.query)
        });

        Ok(results.filter(|r| !r.is_empty()))
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let (enabled, suppress_single) = {
            let config = self.config.read().unwrap();
            let ih = config.inlay_hints.as_ref();
            let enabled = ih.and_then(|s| s.enabled).unwrap_or(true);
            let suppress_single = ih
                .and_then(|s| s.suppress_for_single_arg_calls)
                .unwrap_or(false);
            (enabled, suppress_single)
        };

        if !enabled {
            return Ok(Some(vec![]));
        }

        let hints = self.with_index(|index| {
            providers::inlay_hints::inlay_hints(
                &doc.parsed,
                &doc.line_index,
                index,
                uri,
                suppress_single,
            )
        });

        Ok(Some(hints.unwrap_or_default()))
    }
}

/// Search for the bundled `nwn_script_comp` binary next to our own executable.
fn find_bundled_compiler() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;

    // Try with .exe (Windows)
    let with_exe = dir.join("nwn_script_comp.exe");
    if with_exe.exists() {
        return Some(with_exe);
    }

    // Try without extension (Linux/macOS)
    let without = dir.join("nwn_script_comp");
    if without.exists() {
        return Some(without);
    }

    None
}

/// Default directory names to exclude from source scanning.
fn default_exclude_dirs() -> Vec<String> {
    ["node_modules", "target", "build", "output"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Extract vanilla .nss scripts from the NWN:EE installation into a cache dir.
///
/// Returns (cache directory, NWN root path) if extraction succeeded.
fn extract_vanilla_scripts(nwn_root_setting: Option<&str>) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(nwn_root) = crate::nwn_install::find_nwn_root(nwn_root_setting) else {
        return (None, None);
    };
    tracing::info!("NWN:EE installation found: {}", nwn_root.display());

    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nwscript-lsp")
        .join("vanilla");

    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        tracing::warn!("failed to create vanilla cache dir: {e}");
        return (None, Some(nwn_root));
    }

    match crate::keybif::extract_nss_from_install(&nwn_root) {
        Ok(resources) => {
            let count = resources.len();
            for res in resources {
                let filename = format!("{}.nss", res.resref);
                let dest = cache_dir.join(&filename);
                // Convert to string (most .nss files are ASCII/Latin-1)
                let text = String::from_utf8(res.data.clone())
                    .unwrap_or_else(|_| res.data.iter().map(|&b| b as char).collect());
                if let Err(e) = std::fs::write(&dest, &text) {
                    tracing::warn!("failed to write {}: {e}", filename);
                }
            }
            tracing::info!(
                "extracted {} vanilla .nss scripts to {}",
                count,
                cache_dir.display()
            );
            (Some(cache_dir), Some(nwn_root))
        }
        Err(e) => {
            tracing::warn!("failed to extract vanilla scripts: {e}");
            (None, Some(nwn_root))
        }
    }
}

/// Find the NWN:EE user directory (for compiler --userdirectory flag).
fn find_nwn_home() -> Option<PathBuf> {
    // Check environment variables first
    if let Ok(home) = std::env::var("NWN_HOME") {
        let p = PathBuf::from(&home);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("NWN_USER_DIRECTORY") {
        let p = PathBuf::from(&home);
        if p.is_dir() {
            return Some(p);
        }
    }

    // Platform defaults
    let candidate = if cfg!(windows) {
        dirs::document_dir().map(|d| d.join("Neverwinter Nights"))
    } else if cfg!(target_os = "macos") {
        dirs::document_dir().map(|d| d.join("Neverwinter Nights"))
    } else {
        dirs::data_local_dir().map(|d| d.join("Neverwinter Nights"))
    };

    candidate.filter(|p| p.is_dir())
}
