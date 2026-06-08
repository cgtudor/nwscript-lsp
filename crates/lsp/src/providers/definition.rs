use nwscript_parser::{Declaration, LineIndex};
use tower_lsp::lsp_types::{Location, Position, Url};

use super::symbols::span_to_range;

/// Find the definition of the symbol at the given position.
pub fn goto_definition(
    parsed: &nwscript_parser::ParsedFile,
    source: &str,
    line_index: &LineIndex,
    position: Position,
    uri: &Url,
) -> Option<Location> {
    let offset = line_index.offset(position.line, position.character)?;

    // Find the identifier at this offset
    let target_name = find_ident_at(source, offset as usize)?;

    // Search declarations for a matching definition
    for decl in &parsed.declarations {
        match decl {
            Declaration::Function(f) => {
                if let Some(name) = &f.name {
                    if name.name == target_name {
                        return Some(Location {
                            uri: uri.clone(),
                            range: span_to_range(name.span, line_index),
                        });
                    }
                }
            }
            Declaration::Struct(s) => {
                if let Some(name) = &s.name {
                    if name.name == target_name {
                        return Some(Location {
                            uri: uri.clone(),
                            range: span_to_range(name.span, line_index),
                        });
                    }
                }

                // Check struct fields
                for field in &s.fields {
                    if let Some(fname) = &field.name {
                        if fname.name == target_name {
                            return Some(Location {
                                uri: uri.clone(),
                                range: span_to_range(fname.span, line_index),
                            });
                        }
                    }
                }
            }
            Declaration::GlobalVar(v) => {
                if let Some(name) = &v.name {
                    if name.name == target_name {
                        return Some(Location {
                            uri: uri.clone(),
                            range: span_to_range(name.span, line_index),
                        });
                    }
                }
            }
            Declaration::Include(_) => {}
        }
    }

    None
}

/// Extract the identifier word at the given byte offset.
fn find_ident_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();

    if offset >= bytes.len() {
        return None;
    }

    // Check if we're on an identifier character
    if !is_ident_char(bytes[offset]) {
        return None;
    }

    // Find start of identifier
    let mut start = offset;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    // Find end of identifier
    let mut end = offset;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }

    Some(source[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
