use crate::index::{SymbolInfo, SymbolKind};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

/// Find hover info for a symbol name using the cross-file index.
pub fn hover_for_symbol(sym: &SymbolInfo) -> Hover {
    let content = match sym.kind {
        SymbolKind::Function => {
            let mut s = format!("```nwscript\n{}\n```", sym.detail);
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::Struct => {
            let mut s = format!("```nwscript\n{}\n```", sym.detail);
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::Constant | SymbolKind::Variable => {
            let mut s = format!("```nwscript\n{} {}\n```", sym.detail, sym.name);
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::StructField => {
            format!("```nwscript\n{}\n```", sym.detail)
        }
    };

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: content,
        }),
        range: None,
    }
}

/// Extract the identifier word at the given byte offset in source text.
pub fn find_ident_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();

    if offset >= bytes.len() || !is_ident_char(bytes[offset]) {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }

    Some(source[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
