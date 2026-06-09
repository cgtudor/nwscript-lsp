use std::collections::{HashMap, HashSet};

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

/// Check if a symbol is referenced more than `threshold` times across the workspace.
/// Short-circuits as soon as the threshold is exceeded — much faster than a full count
/// for symbols that are used.
pub fn has_references_beyond(index: &WorkspaceIndex, name: &str, threshold: usize) -> bool {
    let mut count = 0;
    for file_uri in index.all_files() {
        let Some(file) = index.get_file(&file_uri) else {
            continue;
        };
        if !file.source.contains(name) {
            continue;
        }
        count += count_ident_occurrences(&file.source, name);
        if count > threshold {
            return true;
        }
    }
    false
}

/// Count whole-word occurrences of `name` across all indexed files,
/// skipping comments and string literals.
pub fn count_references(index: &WorkspaceIndex, name: &str) -> usize {
    let mut count = 0;
    for file_uri in index.all_files() {
        let Some(file) = index.get_file(&file_uri) else {
            continue;
        };
        if !file.source.contains(name) {
            continue;
        }
        count += count_ident_occurrences(&file.source, name);
    }
    count
}

/// Count whole-word occurrences of `name` in `source`, skipping comments
/// and string literals.
fn count_ident_occurrences(source: &str, name: &str) -> usize {
    let bytes = source.as_bytes();
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    let mut count = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' { i += 1; }
                i += 1;
            }
            if i < bytes.len() { i += 1; }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            i += 2;
            continue;
        }
        if i + name_len <= bytes.len() && &bytes[i..i + name_len] == name_bytes {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok = i + name_len >= bytes.len() || !is_ident_char(bytes[i + name_len]);
            if before_ok && after_ok {
                count += 1;
                i += name_len;
                continue;
            }
        }
        i += 1;
    }
    count
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

/// Count whole-word occurrences of multiple names across all indexed files in a single pass.
/// Scans each file once for all names — O(total_source) instead of O(N * total_source).
pub fn count_references_batch(index: &WorkspaceIndex, names: &[&str]) -> HashMap<String, usize> {
    let name_set: HashSet<&str> = names.iter().copied().collect();
    let mut counts: HashMap<String, usize> =
        name_set.iter().map(|&n| (n.to_string(), 0)).collect();

    if name_set.is_empty() {
        return counts;
    }

    for file_uri in index.all_files() {
        let Some(file) = index.get_file(&file_uri) else {
            continue;
        };

        // Quick substring check per name — skip files that contain none of them
        let relevant: HashSet<&str> = name_set
            .iter()
            .filter(|&&name| file.source.contains(name))
            .copied()
            .collect();
        if relevant.is_empty() {
            continue;
        }

        count_idents_batch(&file.source, &relevant, &mut counts);
    }

    counts
}

/// Scan source once, counting whole-word occurrences of all names in `names`.
/// Skips comments and string literals.
fn count_idents_batch(source: &str, names: &HashSet<&str>, counts: &mut HashMap<String, usize>) {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip string literals
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        // Skip line comments
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
            i += 2;
            continue;
        }
        // Extract identifiers and check against name set
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &source[start..i];
            if names.contains(ident) {
                if let Some(count) = counts.get_mut(ident) {
                    *count += 1;
                }
            }
            continue;
        }
        i += 1;
    }
}
