use dashmap::DashMap;
use nwscript_parser::{LineIndex, ParsedFile};
use ropey::Rope;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;

/// State for a single open document.
pub struct Document {
    pub uri: Url,
    pub version: i32,
    pub rope: Rope,
    pub source: String,
    pub line_index: LineIndex,
    pub parsed: ParsedFile,
}

impl Document {
    pub fn new(uri: Url, version: i32, text: String) -> Self {
        let rope = Rope::from_str(&text);
        let line_index = LineIndex::new(&text);
        let parsed = nwscript_parser::parse(&text);
        Self {
            uri,
            version,
            rope,
            source: text,
            line_index,
            parsed,
        }
    }

    pub fn update(&mut self, version: i32, text: String) {
        self.version = version;
        self.rope = Rope::from_str(&text);
        self.line_index = LineIndex::new(&text);
        self.parsed = nwscript_parser::parse(&text);
        self.source = text;
    }
}

/// Thread-safe store of all open documents.
pub struct DocumentStore {
    documents: DashMap<Url, Arc<Document>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: DashMap::new(),
        }
    }

    pub fn open(&self, uri: Url, version: i32, text: String) {
        let doc = Arc::new(Document::new(uri.clone(), version, text));
        self.documents.insert(uri, doc);
    }

    pub fn update(&self, uri: &Url, version: i32, text: String) {
        let doc = Arc::new(Document::new(uri.clone(), version, text));
        self.documents.insert(uri.clone(), doc);
    }

    pub fn close(&self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<Arc<Document>> {
        self.documents.get(uri).map(|d| Arc::clone(d.value()))
    }

    pub fn all_uris(&self) -> Vec<Url> {
        self.documents.iter().map(|r| r.key().clone()).collect()
    }
}
