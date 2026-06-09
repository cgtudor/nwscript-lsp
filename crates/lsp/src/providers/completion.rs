use std::collections::HashSet;

use crate::index::{SymbolInfo, SymbolKind, WorkspaceIndex};
use nwscript_parser::ast::*;
use nwscript_parser::{LineIndex, ParsedFile};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat, Position, Range, TextEdit, Url,
};

/// Build completion items from ALL workspace symbols plus local variables,
/// with auto-import for symbols not in the current include tree.
pub fn completions_from_index(
    index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    line_index: &LineIndex,
    cursor_offset: u32,
) -> Vec<CompletionItem> {
    let visible = index.visible_symbols(uri);
    let all = index.all_workspace_symbols();

    let mut items = Vec::new();
    let mut seen = HashSet::new();

    // First: local variables and function parameters (highest priority).
    // These sort first because sort_text starts with "0".
    let locals = collect_locals(parsed, cursor_offset);
    for local in &locals {
        if !seen.insert(local.name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: local.name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(local.detail.clone()),
            sort_text: Some(format!("0_{}", local.name)),
            ..Default::default()
        });
    }

    // Then add visible symbols (no import needed)
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
    // Sort priority: 1_ for visible symbols, 2_ for auto-import.
    // (Locals use 0_ — see completions_from_index.)
    let sort_text = if auto_import.is_some() {
        Some(format!("2_{}", sym.name))
    } else {
        Some(format!("1_{}", sym.name))
    };
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
        sort_text,
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

/// Check if a name is a local variable or parameter at the given cursor offset.
pub fn is_local_symbol(parsed: &ParsedFile, cursor_offset: u32, name: &str) -> bool {
    collect_locals(parsed, cursor_offset)
        .iter()
        .any(|l| l.name == name)
}

// =============================================================================
// Local variable extraction
// =============================================================================

struct LocalVar {
    name: String,
    detail: String,
}

fn type_display(ty: &TypeRef) -> String {
    match &ty.kind {
        TypeKind::Void => "void".into(),
        TypeKind::Int => "int".into(),
        TypeKind::Float => "float".into(),
        TypeKind::String => "string".into(),
        TypeKind::Object => "object".into(),
        TypeKind::Vector => "vector".into(),
        TypeKind::Action => "action".into(),
        TypeKind::Effect => "effect".into(),
        TypeKind::Event => "event".into(),
        TypeKind::ItemProperty => "itemproperty".into(),
        TypeKind::Location => "location".into(),
        TypeKind::Talent => "talent".into(),
        TypeKind::Json => "json".into(),
        TypeKind::SqlQuery => "sqlquery".into(),
        TypeKind::Cassowary => "cassowary".into(),
        TypeKind::Struct(name) => format!("struct {name}"),
        TypeKind::Error => "?".into(),
    }
}

/// Collect local variables and parameters visible at the given cursor offset.
///
/// Finds the enclosing function, collects its parameters, then walks the body
/// collecting VarDecl statements that appear before the cursor.
fn collect_locals(parsed: &ParsedFile, cursor_offset: u32) -> Vec<LocalVar> {
    // Find the function whose body contains the cursor
    let func = parsed.declarations.iter().find_map(|decl| {
        if let Declaration::Function(f) = decl {
            if let Some(body) = &f.body {
                if body.span.start <= cursor_offset && cursor_offset <= body.span.end {
                    return Some(f);
                }
            }
        }
        None
    });

    let Some(func) = func else {
        return Vec::new();
    };

    let mut locals = Vec::new();

    // Add function parameters
    for param in &func.params {
        if let Some(name) = &param.name {
            locals.push(LocalVar {
                name: name.name.clone(),
                detail: format!("(parameter) {} {}", type_display(&param.ty), name.name),
            });
        }
    }

    // Walk the function body collecting variable declarations before cursor
    if let Some(body) = &func.body {
        collect_vars_from_block(body, cursor_offset, &mut locals);
    }

    locals
}

/// Recursively collect variable declarations from a block and nested blocks,
/// but only those whose declaration starts before the cursor.
fn collect_vars_from_block(block: &Block, cursor_offset: u32, out: &mut Vec<LocalVar>) {
    for stmt in &block.stmts {
        // Only collect variables declared before the cursor
        if stmt.span().start >= cursor_offset {
            break;
        }
        collect_vars_from_stmt(stmt, cursor_offset, out);
    }
}

fn collect_vars_from_stmt(stmt: &Stmt, cursor_offset: u32, out: &mut Vec<LocalVar>) {
    match stmt {
        Stmt::VarDecl(v) => {
            if v.span.start < cursor_offset {
                if let Some(name) = &v.name {
                    let prefix = if v.is_const { "(const) " } else { "(local) " };
                    out.push(LocalVar {
                        name: name.name.clone(),
                        detail: format!("{}{} {}", prefix, type_display(&v.ty), name.name),
                    });
                }
            }
        }
        Stmt::Block(b) => {
            // Only enter nested blocks if the cursor is inside them
            if b.span.start <= cursor_offset && cursor_offset <= b.span.end {
                collect_vars_from_block(b, cursor_offset, out);
            }
        }
        Stmt::If(s) => {
            collect_vars_from_stmt(&s.then_branch, cursor_offset, out);
            if let Some(else_branch) = &s.else_branch {
                collect_vars_from_stmt(else_branch, cursor_offset, out);
            }
        }
        Stmt::While(s) => {
            collect_vars_from_stmt(&s.body, cursor_offset, out);
        }
        Stmt::DoWhile(s) => {
            collect_vars_from_stmt(&s.body, cursor_offset, out);
        }
        Stmt::For(s) => {
            // For-loop init can declare a variable
            if let Some(init) = &s.init {
                collect_vars_from_stmt(init, cursor_offset, out);
            }
            collect_vars_from_stmt(&s.body, cursor_offset, out);
        }
        Stmt::Switch(s) => {
            for case in &s.cases {
                for case_stmt in &case.stmts {
                    if case_stmt.span().start >= cursor_offset {
                        break;
                    }
                    collect_vars_from_stmt(case_stmt, cursor_offset, out);
                }
            }
        }
        _ => {}
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
