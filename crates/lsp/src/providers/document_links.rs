use nwscript_parser::{LineIndex, ParsedFile};
use tower_lsp::lsp_types::{DocumentLink, Position, Range};

use crate::index::WorkspaceIndex;

/// Produce clickable links for `#include "filename"` directives.
pub fn document_links(
    parsed: &ParsedFile,
    line_index: &LineIndex,
    index: &WorkspaceIndex,
) -> Vec<DocumentLink> {
    let mut links = Vec::new();

    for decl in &parsed.declarations {
        let nwscript_parser::Declaration::Include(inc) = decl else {
            continue;
        };
        let Some(path) = &inc.path else {
            continue;
        };
        let Some(path_span) = &inc.path_span else {
            continue;
        };

        let Some(target_uri) = index.resolve_include(path) else {
            continue;
        };

        let (sl, sc) = line_index.line_col(path_span.start);
        let (el, ec) = line_index.line_col(path_span.end);

        links.push(DocumentLink {
            range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
            target: Some(target_uri),
            tooltip: Some(format!("Open {}.nss", path)),
            data: None,
        });
    }

    links
}
