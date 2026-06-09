use std::collections::HashSet;

use crate::index::WorkspaceIndex;
use nwscript_parser::{Declaration, LineIndex, ParsedFile, Span};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Diagnostic, DiagnosticSeverity, DiagnosticTag,
    Position, Range, TextEdit, Url, WorkspaceEdit,
};

/// Result of analyzing imports: diagnostics for unused ones + quickfix actions.
pub struct ImportAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub actions: Vec<CodeAction>,
}

/// Analyze #include directives: find unused ones, produce grayed-out diagnostics
/// and quickfix code actions to remove them.
pub fn analyze_imports(
    index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
) -> ImportAnalysis {
    let mut diagnostics = Vec::new();
    let mut actions = Vec::new();

    let used_idents = collect_used_identifiers(source);

    for decl in &parsed.declarations {
        let Declaration::Include(inc) = decl else {
            continue;
        };
        let Some(inc_name) = &inc.path else {
            continue;
        };

        // Get symbols from this include
        let Some(inc_uri) = index.resolve_include(inc_name) else {
            continue;
        };
        let Some(inc_file) = index.get_file(&inc_uri) else {
            continue;
        };

        // Check if ANY symbol from the included file is referenced in our source
        let is_used = inc_file.symbols.iter().any(|sym| {
            used_idents.contains(sym.name.as_str())
        });

        if !is_used {
            let line_range = include_line_range(inc.span, line_index, source);
            let diag_range = include_directive_range(inc.span, line_index);

            // Gray-out diagnostic with Unnecessary tag
            let diag = Diagnostic {
                range: diag_range,
                severity: Some(DiagnosticSeverity::HINT),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                source: Some("nwscript-lsp".into()),
                message: format!("Unused import \"{}\"", inc_name),
                ..Default::default()
            };
            diagnostics.push(diag.clone());

            // Quickfix code action to remove the line
            let edit = TextEdit {
                range: line_range,
                new_text: String::new(),
            };

            let mut changes = std::collections::HashMap::new();
            changes.insert(uri.clone(), vec![edit]);

            actions.push(CodeAction {
                title: format!("Remove unused import \"{}\"", inc_name),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            });
        }
    }

    // "Remove all unused imports" action when there are 2+
    let unused_count = diagnostics.len();
    if unused_count >= 2 {
        let all_edits: Vec<TextEdit> = actions
            .iter()
            .filter_map(|a| {
                a.edit
                    .as_ref()?
                    .changes
                    .as_ref()?
                    .get(uri)?
                    .first()
                    .cloned()
            })
            .collect();

        let mut changes = std::collections::HashMap::new();
        changes.insert(uri.clone(), all_edits);

        actions.push(CodeAction {
            title: format!("Remove all {unused_count} unused imports"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(diagnostics.clone()),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            is_preferred: Some(false),
            ..Default::default()
        });
    }

    ImportAnalysis {
        diagnostics,
        actions,
    }
}

/// Collect all identifier-like words from source (rough token scan).
fn collect_used_identifiers(source: &str) -> HashSet<&str> {
    let mut idents = HashSet::new();
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip to start of identifier
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            idents.insert(&source[start..i]);
        } else if bytes[i] == b'"' {
            // Skip strings
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
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Skip line comments
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            // Skip block comments
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    idents
}

/// Get the range of the #include directive text (for graying out).
fn include_directive_range(span: Span, line_index: &LineIndex) -> Range {
    let (sl, sc) = line_index.line_col(span.start);
    let (el, ec) = line_index.line_col(span.end);
    Range::new(Position::new(sl, sc), Position::new(el, ec))
}

/// Get the range covering the entire #include line (including the newline).
fn include_line_range(span: Span, line_index: &LineIndex, source: &str) -> Range {
    let (start_line, _) = line_index.line_col(span.start);
    // Find the end of the line (after the closing quote and any whitespace)
    let line_start_offset = line_index.offset(start_line, 0).unwrap_or(span.start);
    let mut end = line_start_offset as usize;
    let bytes = source.as_bytes();
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1; // include the newline
    }

    let (end_line, end_col) = line_index.line_col(end as u32);
    Range::new(
        Position::new(start_line, 0),
        Position::new(end_line, end_col),
    )
}
