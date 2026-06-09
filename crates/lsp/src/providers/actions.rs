use std::collections::HashSet;

use crate::index::WorkspaceIndex;
use nwscript_parser::ast::*;
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

// =============================================================================
// Unused function detection
// =============================================================================

/// Result of analyzing functions for unused declarations.
pub struct FunctionAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub actions: Vec<CodeAction>,
}

/// Analyze functions in the file: find ones that are never referenced anywhere
/// in the workspace. Uses batch reference counting to scan the workspace once
/// for all function names instead of once per function.
pub fn analyze_unused_functions(
    index: &crate::index::WorkspaceIndex,
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    uri: &Url,
) -> FunctionAnalysis {
    let mut diagnostics = Vec::new();
    let mut actions = Vec::new();

    // Phase 1: collect candidate functions
    struct Candidate {
        name: String,
        name_span: Span,
        fn_span: Span,
        self_occurrences: usize,
        is_prototype: bool,
        has_prototype: bool,
        has_definition: bool,
    }
    let mut candidates: Vec<Candidate> = Vec::new();

    for decl in &parsed.declarations {
        let Declaration::Function(f) = decl else {
            continue;
        };
        let Some(name) = &f.name else {
            continue;
        };
        if matches!(name.name.as_str(), "main" | "StartingConditional") {
            continue;
        }
        if name.name.starts_with('_') {
            continue;
        }

        let is_prototype = f.body.is_none();
        let has_definition = parsed.declarations.iter().any(|d| {
            if let Declaration::Function(other) = d {
                other.body.is_some()
                    && other.name.as_ref().map(|n| &n.name) == Some(&name.name)
            } else {
                false
            }
        });
        let has_prototype = parsed.declarations.iter().any(|d| {
            if let Declaration::Function(other) = d {
                other.body.is_none()
                    && other.name.as_ref().map(|n| &n.name) == Some(&name.name)
            } else {
                false
            }
        });

        if is_prototype && has_definition {
            continue;
        }

        let self_occurrences = if has_prototype && has_definition {
            2
        } else {
            1
        };

        candidates.push(Candidate {
            name: name.name.clone(),
            name_span: name.span,
            fn_span: f.span,
            self_occurrences,
            is_prototype,
            has_prototype,
            has_definition,
        });
    }

    if candidates.is_empty() {
        return FunctionAnalysis {
            diagnostics,
            actions,
        };
    }

    // Phase 2: batch count all references in a single workspace scan
    let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
    let counts = super::references::count_references_batch(index, &names);

    // Phase 3: check each candidate against its count
    for candidate in &candidates {
        let count = counts.get(&candidate.name).copied().unwrap_or(0);
        if count > candidate.self_occurrences {
            continue;
        }

        let (sl, sc) = line_index.line_col(candidate.name_span.start);
        let (el, ec) = line_index.line_col(candidate.name_span.end);

        let diag = Diagnostic {
            range: Range::new(Position::new(sl, sc), Position::new(el, ec)),
            severity: Some(DiagnosticSeverity::HINT),
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            source: Some("nwscript-lsp".into()),
            message: format!("Unused function \"{}\"", candidate.name),
            ..Default::default()
        };
        diagnostics.push(diag.clone());

        // Collect all spans to remove (definition + prototype if both exist)
        let mut edits = Vec::new();
        let removal_range = multiline_span_range(candidate.fn_span, line_index, source);
        edits.push(TextEdit {
            range: removal_range,
            new_text: String::new(),
        });

        let remove_counterpart = (candidate.is_prototype && candidate.has_definition)
            || (!candidate.is_prototype && candidate.has_prototype);
        if remove_counterpart {
            for other_decl in &parsed.declarations {
                if let Declaration::Function(other) = other_decl {
                    if other.span.start == candidate.fn_span.start {
                        continue; // skip self
                    }
                    if other.name.as_ref().map(|n| &n.name) == Some(&candidate.name) {
                        let other_range =
                            multiline_span_range(other.span, line_index, source);
                        edits.push(TextEdit {
                            range: other_range,
                            new_text: String::new(),
                        });
                    }
                }
            }
        }

        let mut changes = std::collections::HashMap::new();
        changes.insert(uri.clone(), edits);

        actions.push(CodeAction {
            title: format!("Remove unused function \"{}\"", candidate.name),
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

    FunctionAnalysis {
        diagnostics,
        actions,
    }
}

// =============================================================================
// Import analysis helpers
// =============================================================================

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

// =============================================================================
// Unused variable detection
// =============================================================================

/// Result of analyzing local variables for unused declarations.
pub struct VariableAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub actions: Vec<CodeAction>,
}

/// Analyze local variables in each function: find unused ones and produce
/// grayed-out diagnostics + quickfix code actions to remove them.
pub fn analyze_unused_variables(
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    uri: &Url,
) -> VariableAnalysis {
    let mut diagnostics = Vec::new();
    let mut actions = Vec::new();

    for decl in &parsed.declarations {
        let Declaration::Function(f) = decl else {
            continue;
        };
        let Some(body) = &f.body else {
            continue;
        };

        // Collect all local variable declarations in this function
        let mut local_decls: Vec<LocalDecl> = Vec::new();
        collect_local_decls(body, &mut local_decls);

        // Also collect parameters
        for param in &f.params {
            if let Some(name) = &param.name {
                local_decls.push(LocalDecl {
                    name: name.name.clone(),
                    name_span: name.span,
                    decl_span: param.span,
                    is_param: true,
                });
            }
        }

        // Collect all identifier-like words in the function body (skipping
        // strings and comments), then check each local against that set.
        let body_source = &source[body.span.start as usize..body.span.end as usize];
        let body_idents = collect_used_identifiers(body_source);

        for local in &local_decls {
            // Skip the underscore convention (intentionally unused)
            if local.name.starts_with('_') {
                continue;
            }

            // Parameters are outside the body text, so if their name appears
            // at all in the body, they're used. For local vars, the declaration
            // is inside the body so the name appears in the set regardless;
            // we need to check for usage beyond the declaration by counting.
            let is_used = if local.is_param {
                body_idents.contains(local.name.as_str())
            } else {
                // The declaration puts the name in the ident set. Check if
                // it appears more than once via a targeted count.
                let decl_start = local.decl_span.start as usize;
                let decl_end = local.decl_span.end as usize;
                // Check if the name appears anywhere outside the declaration span
                let before_decl = if decl_start > body.span.start as usize {
                    let pre = &source[body.span.start as usize..decl_start];
                    collect_used_identifiers(pre).contains(local.name.as_str())
                } else {
                    false
                };
                let after_decl = if decl_end < body.span.end as usize {
                    let post = &source[decl_end..body.span.end as usize];
                    collect_used_identifiers(post).contains(local.name.as_str())
                } else {
                    false
                };
                before_decl || after_decl
            };

            if !is_used {
                let (sl, sc) = line_index.line_col(local.name_span.start);
                let (el, ec) = line_index.line_col(local.name_span.end);
                let diag_range = Range::new(Position::new(sl, sc), Position::new(el, ec));

                let kind_label = if local.is_param { "parameter" } else { "variable" };

                let diag = Diagnostic {
                    range: diag_range,
                    severity: Some(DiagnosticSeverity::HINT),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    source: Some("nwscript-lsp".into()),
                    message: format!("Unused {} \"{}\"", kind_label, local.name),
                    ..Default::default()
                };
                diagnostics.push(diag.clone());

                // Only offer quickfix removal for local variables (not params)
                if !local.is_param {
                    let removal_range =
                        statement_line_range(local.decl_span, line_index, source);

                    let edit = TextEdit {
                        range: removal_range,
                        new_text: String::new(),
                    };

                    let mut changes = std::collections::HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);

                    actions.push(CodeAction {
                        title: format!("Remove unused variable \"{}\"", local.name),
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
        }
    }

    VariableAnalysis {
        diagnostics,
        actions,
    }
}

struct LocalDecl {
    name: String,
    name_span: Span,
    decl_span: Span,
    is_param: bool,
}

/// Collect all local variable declarations from a block (recursively).
fn collect_local_decls(block: &Block, out: &mut Vec<LocalDecl>) {
    for stmt in &block.stmts {
        collect_local_decls_from_stmt(stmt, out);
    }
}

fn collect_local_decls_from_stmt(stmt: &Stmt, out: &mut Vec<LocalDecl>) {
    match stmt {
        Stmt::VarDecl(v) => {
            if let Some(name) = &v.name {
                out.push(LocalDecl {
                    name: name.name.clone(),
                    name_span: name.span,
                    decl_span: v.span,
                    is_param: false,
                });
            }
        }
        Stmt::Block(b) => collect_local_decls(b, out),
        Stmt::If(s) => {
            collect_local_decls_from_stmt(&s.then_branch, out);
            if let Some(e) = &s.else_branch {
                collect_local_decls_from_stmt(e, out);
            }
        }
        Stmt::While(s) => collect_local_decls_from_stmt(&s.body, out),
        Stmt::DoWhile(s) => collect_local_decls_from_stmt(&s.body, out),
        Stmt::For(s) => {
            if let Some(init) = &s.init {
                collect_local_decls_from_stmt(init, out);
            }
            collect_local_decls_from_stmt(&s.body, out);
        }
        Stmt::Switch(s) => {
            for case in &s.cases {
                for cs in &case.stmts {
                    collect_local_decls_from_stmt(cs, out);
                }
            }
        }
        _ => {}
    }
}

/// Count whole-word occurrences of `name` in a source range,
/// skipping strings and comments.
fn count_ident_in_range(source: &str, name: &str) -> usize {
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
            let before_ok = i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let after_ok = i + name_len >= bytes.len()
                || !(bytes[i + name_len].is_ascii_alphanumeric() || bytes[i + name_len] == b'_');
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

/// Get range covering the full line of a statement (for removal).
/// Get range covering the full lines of a span (for single-line removal like variable decls).
fn statement_line_range(span: Span, line_index: &LineIndex, source: &str) -> Range {
    let (start_line, _) = line_index.line_col(span.start);
    let line_start_offset = line_index.offset(start_line, 0).unwrap_or(span.start);
    let mut end = line_start_offset as usize;
    let bytes = source.as_bytes();
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1;
    }
    let (end_line, end_col) = line_index.line_col(end as u32);
    Range::new(
        Position::new(start_line, 0),
        Position::new(end_line, end_col),
    )
}

/// Get range covering all lines of a multi-line span (for function removal).
/// Covers from the start of the first line to after the newline of the last line.
fn multiline_span_range(span: Span, line_index: &LineIndex, source: &str) -> Range {
    let (start_line, _) = line_index.line_col(span.start);
    // Find the end of the last line of the span
    let mut end = span.end as usize;
    let bytes = source.as_bytes();
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1;
    }
    let (end_line, end_col) = line_index.line_col(end as u32);
    Range::new(
        Position::new(start_line, 0),
        Position::new(end_line, end_col),
    )
}

// =============================================================================
// Helper functions
// =============================================================================

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
