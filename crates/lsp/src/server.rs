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

/// Configuration received from the client.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NwscriptConfig {
    pub compiler_path: Option<String>,
    pub include_dirs: Option<Vec<String>>,
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

        // Also add any extra include dirs from config
        {
            let config = self.config.read().unwrap();
            if let Some(extra) = &config.include_dirs {
                for dir in extra {
                    let p = PathBuf::from(dir);
                    if p.is_dir() && !source_dirs.contains(&p) {
                        source_dirs.push(p);
                    }
                }
            }
        }

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
        let nss_dirs = crate::diagnostics::collect_nss_directories(&source_dirs);
        tracing::info!("found {} directories containing .nss files", nss_dirs.len());
        *self.nss_dirs.write().unwrap() = nss_dirs;

        let index = WorkspaceIndex::new(source_dirs, workspace_dirs.to_vec());
        index.scan_workspace();

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

        let compiler_diagnostics = crate::diagnostics::compile_file(
            &compiler_path,
            &file_path,
            &source,
            &nasher_cache,
            &fallback_dirs,
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
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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
        let Some(doc) = self.documents.get(uri) else {
            return Ok(Some(CompletionResponse::Array(
                providers::completion::keyword_completions(),
            )));
        };

        let guard = self.index.read().unwrap();
        let items = match guard.as_ref() {
            Some(index) => {
                let mut items = providers::completion::completions_from_index(
                    index, uri, &doc.parsed, &doc.line_index,
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
