use crate::index::WorkspaceIndex;
use nwscript_parser::LineIndex;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

/// Find all references to the symbol at the given position across the workspace.
///
/// Scans all indexed files for whole-word occurrences of the identifier,
/// skipping matches inside comments and string literals.
pub fn find_references(
    index: &WorkspaceIndex,
    source: &str,
    line_index: &LineIndex,
    position: Position,
    uri: &Url,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(offset) = line_index.offset(position.line, position.character) else {
        return Vec::new();
    };
    let Some(target_name) = super::hover::find_ident_at(source, offset as usize) else {
        return Vec::new();
    };

    // Verify the symbol actually exists in the index
    let symbols = index.visible_symbols(uri);
    let is_known_symbol = symbols.iter().any(|s| s.name == target_name);
    if !is_known_symbol {
        // Still search — might be a local variable or parameter
    }

    let mut locations = Vec::new();

    // Search all indexed files. Fast-path: skip files whose source doesn't
    // contain the name at all (substring check is much cheaper than the full
    // word-boundary + comment-skipping scan).
    for file_uri in index.all_files() {
        let Some(file) = index.get_file(&file_uri) else {
            continue;
        };

        if !file.source.contains(&target_name) {
            continue;
        }

        find_ident_occurrences(
            &file.source,
            &target_name,
            &file.line_index,
            &file_uri,
            include_declaration,
            &mut locations,
        );
    }

    locations
}

/// Find all whole-word occurrences of `name` in `source`, skipping comments
/// and string literals. Appends `Location`s to `out`.
fn find_ident_occurrences(
    source: &str,
    name: &str,
    line_index: &LineIndex,
    uri: &Url,
    _include_declaration: bool,
    out: &mut Vec<Location>,
) {
    let bytes = source.as_bytes();
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();

    if name_len == 0 || bytes.len() < name_len {
        return;
    }

    let mut i = 0;
    while i < bytes.len() {
        // Skip string literals
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // skip closing quote
            }
            continue;
        }

        // Skip single-line comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Skip block comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        // Check for identifier match
        if i + name_len <= bytes.len() && &bytes[i..i + name_len] == name_bytes {
            // Verify whole word: not preceded or followed by ident chars
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok =
                i + name_len >= bytes.len() || !is_ident_char(bytes[i + name_len]);

            if before_ok && after_ok {
                let (sl, sc) = line_index.line_col(i as u32);
                let (el, ec) = line_index.line_col((i + name_len) as u32);
                out.push(Location {
                    uri: uri.clone(),
                    range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
                });
                i += name_len;
                continue;
            }
        }

        i += 1;
    }
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
