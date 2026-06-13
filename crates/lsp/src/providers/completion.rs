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
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    // First: local variables and function parameters (highest priority).
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

    // Pre-compute the include tree set once (instead of per-symbol).
    let include_tree = index.include_tree_set(uri);
    let import_insert_pos = find_import_insert_position(parsed, line_index);

    // Iterate all workspace symbols by reference (no mass clone).
    // Symbols in the include tree are "visible" (sort priority 1_),
    // others need auto-import (sort priority 2_).
    index.for_each_symbol(|sym| {
        if sym.kind == SymbolKind::StructField {
            return;
        }
        if !seen.insert(sym.name.clone()) {
            return;
        }

        let in_tree = sym.include_name.is_empty()
            || include_tree.contains(&sym.include_name.to_lowercase());

        if in_tree {
            items.push(symbol_to_completion(sym, None));
        } else {
            let import_edit = TextEdit {
                range: Range::new(import_insert_pos, import_insert_pos),
                new_text: format!("#include \"{}\"\n", sym.include_name),
            };
            items.push(symbol_to_completion(sym, Some(import_edit)));
        }
    });

    items
}

/// Produce struct field completions when the cursor is in a member-access
/// context (`someVar.` or a chain like `a.b.c.`).
///
/// Returns `None` if the cursor is not after a `.` whose left-hand side
/// resolves to a struct type — in that case the caller should fall back to the
/// normal completion list.
pub fn struct_field_completions(
    index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    source: &str,
    cursor_offset: u32,
) -> Option<Vec<CompletionItem>> {
    let chain = parse_member_access(source, cursor_offset)?;
    let (base, fields) = chain.split_first()?;

    // Resolve the struct type of the base identifier (local/param, then global).
    let base_type = find_local_type(parsed, cursor_offset, base)
        .or_else(|| index.global_var_type(uri, base))?;
    let mut struct_name = match base_type {
        TypeKind::Struct(name) => name,
        _ => return None,
    };

    // Walk intermediate field accesses; each must itself be a struct.
    for field in fields {
        let struct_fields = index.struct_fields(uri, &struct_name)?;
        let (_, ty) = struct_fields.into_iter().find(|(n, _)| n == field)?;
        struct_name = match ty {
            TypeKind::Struct(name) => name,
            _ => return None,
        };
    }

    let target_fields = index.struct_fields(uri, &struct_name)?;
    let items = target_fields
        .into_iter()
        .map(|(fname, ty)| {
            let ty_display = crate::providers::symbols::format_type(&ty);
            CompletionItem {
                label: fname.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("(field) {ty_display} {fname}")),
                sort_text: Some(format!("0_{fname}")),
                ..Default::default()
            }
        })
        .collect();
    Some(items)
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws_back(bytes: &[u8], i: &mut usize) {
    while *i > 0 && bytes[*i - 1].is_ascii_whitespace() {
        *i -= 1;
    }
}

/// Scan backwards from the cursor to detect a member-access chain.
///
/// For `a.b.c.fo<cursor>` (or `a.b.c.<cursor>`) returns `["a", "b", "c"]`.
/// Returns `None` when the cursor is not immediately after a `.` that follows a
/// plain identifier chain (e.g. after a function call `).` or a numeric literal).
fn parse_member_access(source: &str, cursor_offset: u32) -> Option<Vec<String>> {
    let bytes = source.as_bytes();
    let mut i = (cursor_offset as usize).min(bytes.len());

    // Skip the partial field name being typed after the dot.
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    skip_ws_back(bytes, &mut i);

    // Must be immediately after a `.`.
    if i == 0 || bytes[i - 1] != b'.' {
        return None;
    }
    i -= 1;

    let mut chain: Vec<String> = Vec::new();
    loop {
        skip_ws_back(bytes, &mut i);
        let end = i;
        while i > 0 && is_ident_byte(bytes[i - 1]) {
            i -= 1;
        }
        if i == end {
            return None; // not a plain identifier (call, index, etc.)
        }
        if bytes[i].is_ascii_digit() {
            return None; // numeric literal like `1.5`, not a member access
        }
        chain.push(source[i..end].to_string());

        skip_ws_back(bytes, &mut i);
        if i > 0 && bytes[i - 1] == b'.' {
            i -= 1;
            continue;
        }
        break;
    }
    chain.reverse();
    Some(chain)
}

/// Find the declared type of a local variable or parameter by name at the
/// given cursor offset.
fn find_local_type(parsed: &ParsedFile, cursor_offset: u32, name: &str) -> Option<TypeKind> {
    collect_locals(parsed, cursor_offset)
        .into_iter()
        .find(|l| l.name == name)
        .map(|l| l.ty)
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

/// Find a local variable or parameter by name at the given cursor offset.
/// Returns the detail string (e.g. "(parameter) object oPC") if found.
pub fn find_local_detail(parsed: &ParsedFile, cursor_offset: u32, name: &str) -> Option<String> {
    collect_locals(parsed, cursor_offset)
        .into_iter()
        .find(|l| l.name == name)
        .map(|l| l.detail)
}

// =============================================================================
// Local variable extraction
// =============================================================================

struct LocalVar {
    name: String,
    detail: String,
    ty: TypeKind,
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
                ty: param.ty.kind.clone(),
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
                        ty: v.ty.kind.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::WorkspaceIndex;

    /// Build a single-file index and compute field completions at the cursor
    /// marked by `|` in `source`.
    fn fields_at(source: &str) -> Option<Vec<String>> {
        let cursor_offset = source.find('|').expect("test source needs a `|` cursor marker") as u32;
        let clean = source.replacen('|', "", 1);

        let index = WorkspaceIndex::new(vec![], vec![], vec![]);
        let uri = Url::parse("file:///test.nss").unwrap();
        index.update_file(&uri, clean.clone());

        let parsed = nwscript_parser::parse(&clean);
        struct_field_completions(&index, &uri, &parsed, &clean, cursor_offset)
            .map(|items| items.into_iter().map(|i| i.label).collect())
    }

    #[test]
    fn local_struct_field_completion() {
        let fields = fields_at(
            "struct Vec3 { float x; float y; float z; }\n\
             void main() { struct Vec3 v; v.| }",
        )
        .expect("expected field completions");
        assert_eq!(fields, vec!["x", "y", "z"]);
    }

    #[test]
    fn partial_field_name_still_resolves() {
        let fields = fields_at(
            "struct Vec3 { float x; float y; float z; }\n\
             void main() { struct Vec3 v; v.y| }",
        )
        .expect("expected field completions");
        assert_eq!(fields, vec!["x", "y", "z"]);
    }

    #[test]
    fn parameter_struct_field_completion() {
        let fields = fields_at(
            "struct Point { int a; int b; }\n\
             void main(struct Point p) { p.| }",
        )
        .expect("expected field completions");
        assert_eq!(fields, vec!["a", "b"]);
    }

    #[test]
    fn nested_struct_chain() {
        let fields = fields_at(
            "struct Inner { int deep; }\n\
             struct Outer { struct Inner inner; int top; }\n\
             void main() { struct Outer o; o.inner.| }",
        )
        .expect("expected field completions");
        assert_eq!(fields, vec!["deep"]);
    }

    #[test]
    fn global_struct_field_completion() {
        let fields = fields_at(
            "struct Vec3 { float x; float y; float z; }\n\
             struct Vec3 g;\n\
             void main() { g.| }",
        )
        .expect("expected field completions");
        assert_eq!(fields, vec!["x", "y", "z"]);
    }

    #[test]
    fn non_member_access_returns_none() {
        // Bare identifier (no `.`) should not trigger field completion.
        assert!(fields_at("void main() { int x; x| }").is_none());
    }

    #[test]
    fn non_struct_type_returns_none() {
        // `int` has no fields.
        assert!(fields_at("void main() { int n; n.| }").is_none());
    }

    #[test]
    fn unknown_variable_returns_none() {
        assert!(fields_at("void main() { unknownVar.| }").is_none());
    }
}
