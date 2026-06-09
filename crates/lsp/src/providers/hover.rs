use crate::index::{SymbolInfo, SymbolKind};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

/// Build hover info for a workspace symbol (function, struct, constant, variable).
pub fn hover_for_symbol(sym: &SymbolInfo) -> Hover {
    let content = match sym.kind {
        SymbolKind::Function => {
            // Modern IDE style: show return type, function name, and params
            // with default values, formatted like a declaration
            let mut sig = String::new();
            if let (Some(params), Some(ret_ty)) = (&sym.params, &sym.return_type) {
                sig.push_str(ret_ty);
                sig.push(' ');
                sig.push_str(&sym.name);
                sig.push('(');
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        sig.push_str(", ");
                    }
                    sig.push_str(&p.ty);
                    if !p.name.is_empty() {
                        sig.push(' ');
                        sig.push_str(&p.name);
                    }
                    if let Some(default) = &p.default_text {
                        sig.push_str(" = ");
                        sig.push_str(default);
                    }
                }
                sig.push(')');
            } else {
                // Fallback to stored detail
                sig.push_str(&sym.detail);
            }

            let mut s = format!("```nwscript\n{sig}\n```");
            s.push_str(&format!("\n\n*{}*", sym.include_name));
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::Struct => {
            let mut s = format!("```nwscript\nstruct {}\n```", sym.name);
            // Show fields in the detail below
            if !sym.detail.is_empty() && sym.detail != format!("struct {}", sym.name) {
                s.push_str(&format!("\n\n*{}*", sym.include_name));
            }
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::Constant => {
            let ty = sym.detail.strip_prefix("const ").unwrap_or(&sym.detail);
            let value_part = match &sym.initializer_text {
                Some(v) => format!(" = {v}"),
                None => String::new(),
            };
            let mut s = format!("```nwscript\nconst {} {}{}\n```", ty, sym.name, value_part);
            s.push_str(&format!("\n\n*{}*", sym.include_name));
            if let Some(doc) = &sym.doc {
                s.push_str("\n\n---\n\n");
                s.push_str(doc);
            }
            s
        }
        SymbolKind::Variable => {
            let mut s = format!("```nwscript\n{} {}\n```", sym.detail, sym.name);
            s.push_str(&format!("\n\n*{}*", sym.include_name));
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

/// Build hover info for a local variable or parameter.
pub fn hover_for_local(label: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```nwscript\n{label}\n```"),
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
