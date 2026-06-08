use std::collections::HashSet;

use crate::index::{SymbolInfo, SymbolKind, WorkspaceIndex};
use nwscript_parser::{LineIndex, ParsedFile};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat, Position, Range, TextEdit, Url,
};

/// Build completion items from ALL workspace symbols, with auto-import for
/// symbols not in the current include tree.
pub fn completions_from_index(
    index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    line_index: &LineIndex,
) -> Vec<CompletionItem> {
    let visible = index.visible_symbols(uri);
    let all = index.all_workspace_symbols();

    let mut items = Vec::new();
    let mut seen = HashSet::new();

    // First add visible symbols (no import needed)
    for sym in &visible {
        if sym.kind == SymbolKind::StructField {
            continue;
        }
        if !seen.insert(sym.name.clone()) {
            continue;
        }
        items.push(symbol_to_completion(sym, None));
    }

    // Then add workspace symbols not yet visible (need auto-import)
    let import_insert_pos = find_import_insert_position(parsed, line_index);
    for sym in &all {
        if sym.kind == SymbolKind::StructField {
            continue;
        }
        if !seen.insert(sym.name.clone()) {
            continue;
        }
        // This symbol is not visible — needs an import
        let needs_import = !sym.include_name.is_empty()
            && !sym.include_name.eq_ignore_ascii_case("nwscript")
            && !index.is_in_include_tree(uri, &sym.include_name);

        if needs_import {
            let import_edit = TextEdit {
                range: Range::new(import_insert_pos, import_insert_pos),
                new_text: format!("#include \"{}\"\n", sym.include_name),
            };
            items.push(symbol_to_completion(sym, Some(import_edit)));
        } else {
            items.push(symbol_to_completion(sym, None));
        }
    }

    items
}

fn symbol_to_completion(sym: &SymbolInfo, auto_import: Option<TextEdit>) -> CompletionItem {
    let (kind, insert_text, insert_format) = match sym.kind {
        SymbolKind::Function => {
            let snippet = if let Some(params) = &sym.params {
                if params.is_empty() {
                    format!("{}()", sym.name)
                } else {
                    let param_snippets: Vec<String> = params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| format!("${{{}: {}}}", i + 1, p.name))
                        .collect();
                    format!("{}({})", sym.name, param_snippets.join(", "))
                }
            } else {
                format!("{}()", sym.name)
            };
            (
                CompletionItemKind::FUNCTION,
                Some(snippet),
                Some(InsertTextFormat::SNIPPET),
            )
        }
        SymbolKind::Struct => (CompletionItemKind::STRUCT, None, None),
        SymbolKind::Constant => (CompletionItemKind::CONSTANT, None, None),
        SymbolKind::Variable => (CompletionItemKind::VARIABLE, None, None),
        SymbolKind::StructField => (CompletionItemKind::FIELD, None, None),
    };

    let detail = if auto_import.is_some() {
        format!("{} (auto-import {})", sym.detail, sym.include_name)
    } else {
        sym.detail.clone()
    };

    CompletionItem {
        label: sym.name.clone(),
        kind: Some(kind),
        detail: Some(detail),
        insert_text,
        insert_text_format: insert_format,
        additional_text_edits: auto_import.map(|e| vec![e]),
        ..Default::default()
    }
}

/// Find where to insert a new #include line.
/// Returns the position after the last existing #include, or line 0 if none.
fn find_import_insert_position(parsed: &ParsedFile, line_index: &LineIndex) -> Position {
    let mut last_include_end: Option<u32> = None;

    for decl in &parsed.declarations {
        if let nwscript_parser::Declaration::Include(inc) = decl {
            last_include_end = Some(inc.span.end);
        }
    }

    match last_include_end {
        Some(offset) => {
            let (line, _) = line_index.line_col(offset);
            // Insert on the line after the last include
            Position::new(line + 1, 0)
        }
        None => Position::new(0, 0),
    }
}

/// NWScript keyword completions.
pub fn keyword_completions() -> Vec<CompletionItem> {
    let keywords = [
        "void", "int", "float", "string", "object", "struct", "vector",
        "effect", "event", "itemproperty", "location", "talent", "json",
        "action", "sqlquery", "cassowary",
        "if", "else", "while", "for", "do", "switch", "case", "default",
        "break", "continue", "return", "const",
        "TRUE", "FALSE", "OBJECT_SELF", "OBJECT_INVALID",
    ];

    keywords
        .iter()
        .map(|&kw| CompletionItem {
            label: kw.into(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        })
        .collect()
}
