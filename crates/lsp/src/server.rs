use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::document::DocumentStore;
use crate::providers;

/// Configuration received from the client.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NwscriptConfig {
    /// Path to the nwn_script_comp binary.
    pub compiler_path: Option<String>,
    /// Additional include directories for compilation.
    pub include_dirs: Option<Vec<String>>,
}

pub struct NwscriptLanguageServer {
    client: Client,
    documents: DocumentStore,
    config: RwLock<NwscriptConfig>,
}

impl NwscriptLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
            config: RwLock::new(NwscriptConfig::default()),
        }
    }

    /// Publish diagnostics from our parser for a document.
    async fn publish_parser_diagnostics(&self, uri: &Url) {
        let Some(doc) = self.documents.get(uri) else {
            return;
        };

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
                    // Try to find bundled compiler next to our binary
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
}

#[tower_lsp::async_trait]
impl LanguageServer for NwscriptLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
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
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "nwscript-lsp initialized")
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

        // We use FULL sync, so there's exactly one change with the entire text.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.update(&uri, version, change.text);
            self.publish_parser_diagnostics(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // On save, also run the external compiler for precise diagnostics.
        self.run_compiler_diagnostics(&params.text_document.uri)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.close(&params.text_document.uri);
        // Clear diagnostics for closed file.
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        let mut items = providers::completion::completions_from_file(&doc.parsed);
        items.extend(providers::completion::keyword_completions());

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(doc) = self.documents.get(uri) else {
            return Ok(None);
        };

        Ok(providers::hover::hover_at(
            &doc.parsed,
            &doc.source,
            &doc.line_index,
            position,
        ))
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

        let location = providers::definition::goto_definition(
            &doc.parsed,
            &doc.source,
            &doc.line_index,
            position,
            uri,
        );

        Ok(location.map(GotoDefinitionResponse::Scalar))
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
}
