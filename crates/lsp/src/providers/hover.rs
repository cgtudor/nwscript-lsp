use nwscript_parser::{Declaration, LineIndex, Span};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::symbols::format_function_signature;

/// Find hover info at the given position.
pub fn hover_at(
    parsed: &nwscript_parser::ParsedFile,
    source: &str,
    line_index: &LineIndex,
    position: Position,
) -> Option<Hover> {
    let offset = line_index.offset(position.line, position.character)?;

    // Find the token/identifier at this offset.
    // Walk declarations to find what's at this position.
    for decl in &parsed.declarations {
        match decl {
            Declaration::Function(f) => {
                // Hover on function name
                if let Some(name) = &f.name {
                    if contains(name.span, offset) {
                        let sig = format_function_signature(f);
                        let doc = find_leading_comment(source, f.span);
                        let content = format_hover_nwscript(&sig, doc.as_deref());
                        return Some(make_hover(content));
                    }
                }

                // Hover on parameter names
                for param in &f.params {
                    if let Some(name) = &param.name {
                        if contains(name.span, offset) {
                            let ty = super::symbols::format_type(&param.ty.kind);
                            let content = format!("```nwscript\n{ty} {}\n```", name.name);
                            return Some(make_hover(content));
                        }
                    }
                }
            }
            Declaration::Struct(s) => {
                if let Some(name) = &s.name {
                    if contains(name.span, offset) {
                        let fields: Vec<String> = s
                            .fields
                            .iter()
                            .map(|f| {
                                let ty = super::symbols::format_type(&f.ty.kind);
                                let name = f
                                    .name
                                    .as_ref()
                                    .map(|n| n.name.as_str())
                                    .unwrap_or("?");
                                format!("    {ty} {name};")
                            })
                            .collect();
                        let content = format!(
                            "```nwscript\nstruct {} {{\n{}\n}}\n```",
                            name.name,
                            fields.join("\n")
                        );
                        return Some(make_hover(content));
                    }
                }
            }
            Declaration::GlobalVar(v) => {
                if let Some(name) = &v.name {
                    if contains(name.span, offset) {
                        let ty = super::symbols::format_type(&v.ty.kind);
                        let prefix = if v.is_const { "const " } else { "" };
                        let content = format!("```nwscript\n{prefix}{ty} {}\n```", name.name);
                        return Some(make_hover(content));
                    }
                }
            }
            Declaration::Include(_) => {}
        }
    }

    None
}

fn contains(span: Span, offset: u32) -> bool {
    offset >= span.start && offset < span.end
}

fn make_hover(content: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: content,
        }),
        range: None,
    }
}

fn format_hover_nwscript(signature: &str, doc: Option<&str>) -> String {
    let mut s = format!("```nwscript\n{signature}\n```");
    if let Some(doc) = doc {
        s.push_str("\n\n---\n\n");
        s.push_str(doc);
    }
    s
}

/// Extract leading `//` comments before a span as documentation.
fn find_leading_comment(source: &str, span: Span) -> Option<String> {
    let before = &source[..span.start as usize];
    let mut lines: Vec<&str> = Vec::new();

    // Walk backwards through lines before the declaration
    for line in before.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            let doc = trimmed.trim_start_matches('/').trim();
            lines.push(doc);
        } else if trimmed.is_empty() {
            // Allow one blank line gap
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
