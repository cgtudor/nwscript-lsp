use std::path::PathBuf;
use std::sync::RwLock;

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
}

pub struct NwscriptLanguageServer {
    client: Client,
    documents: DocumentStore,
    index: RwLock<Option<WorkspaceIndex>>,
    config: RwLock<NwscriptConfig>,
}

impl NwscriptLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
            index: RwLock::new(None),
            config: RwLock::new(NwscriptConfig::default()),
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

        let index = WorkspaceIndex::new(source_dirs);
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

    /// Publish diagnostics from our parser for a document.
    async fn publish_parser_diagnostics(&self, uri: &Url) {
        let Some(doc) = self.documents.get(uri) else {
            return;
        };

        // Also update the workspace index with the latest source
        {
            let guard = self.index.read().unwrap();
            if let Some(index) = guard.as_ref() {
                index.update_file(uri, doc.source.clone());
            }
        }

        let diagnostics: Vec<Diagnostic> = doc
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

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, Some(doc.version))
            .await;
    }

    /// Run the external compiler for additional diagnostics.
    async fn run_compiler_diagnostics(&self, uri: &Url) {
        let (compiler_path, include_dirs) = {
            let config = self.config.read().unwrap();
            let compiler_path = match &config.compiler_path {
                Some(p) => PathBuf::from(p),
                None => {
                    if let Ok(exe) = std::env::current_exe() {
                        let dir = exe.parent().unwrap_or(exe.as_ref());
                        let bundled = dir.join("nwn_script_comp.exe");
                        if bundled.exists() {
                            bundled
                        } else {
                            PathBuf::from("nwn_script_comp")
                        }
                    } else {
                        PathBuf::from("nwn_script_comp")
                    }
                }
            };
            let include_dirs: Vec<PathBuf> = config
                .include_dirs
                .as_ref()
                .map(|dirs| dirs.iter().map(PathBuf::from).collect())
                .unwrap_or_default();
            (compiler_path, include_dirs)
        };

        let file_path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };

        let diagnostics =
            crate::diagnostics::compile_file(&compiler_path, &file_path, &include_dirs).await;

        if !diagnostics.is_empty() {
            let doc = self.documents.get(uri);
            let version = doc.as_ref().map(|d| d.version);
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, version)
                .await;
        }
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
        self.publish_parser_diagnostics(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.update(&uri, version, change.text);
            self.publish_parser_diagnostics(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.run_compiler_diagnostics(&params.text_document.uri)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.close(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;

        let items = self.with_index(|index| {
            let symbols = index.visible_symbols(uri);
            let mut items = providers::completion::completions_from_symbols(&symbols);
            items.extend(providers::completion::keyword_completions());
            items
        });

        match items {
            Some(items) => Ok(Some(CompletionResponse::Array(items))),
            None => {
                // Fallback: use just the current document
                let doc = self.documents.get(uri);
                if doc.is_some() {
                    Ok(Some(CompletionResponse::Array(
                        providers::completion::keyword_completions(),
                    )))
                } else {
                    Ok(None)
                }
            }
        }
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
}
