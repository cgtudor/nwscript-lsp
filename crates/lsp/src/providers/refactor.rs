use std::collections::{HashMap, HashSet};

use crate::index::{SymbolKind, WorkspaceIndex};
use nwscript_parser::ast::*;
use nwscript_parser::{Declaration, LineIndex, ParsedFile, Span};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Command, CreateFile, DocumentChangeOperation,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    ResourceOp, TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

use super::symbols::span_to_range;

// =============================================================================
// Extract Variable
// =============================================================================

/// Produce an "Extract Variable" code action if the cursor is on or selection
/// covers an expression. Works with:
/// - Cursor on a function name → extracts the whole call expression
/// - Cursor on a compound expression → extracts the smallest non-trivial expression
/// - Selection covering an expression → extracts exactly what's selected
pub fn extract_variable(
    index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    sel_start: u32,
    sel_end: u32,
) -> Option<CodeAction> {
    // Find the enclosing function
    let func = find_enclosing_function(parsed, sel_start)?;
    let body = func.body.as_ref()?;

    // Find the expression: selection-based or cursor-based
    let expr_span = if sel_start == sel_end {
        // Cursor mode: find the smallest non-trivial expression at cursor
        find_extractable_at_cursor(body, sel_start, source)?
    } else {
        // Selection mode: find the expression matching the selection
        find_expression_at(body, sel_start, sel_end)?
    };

    // Don't extract trivial things
    let expr_text = expr_span.text(source);
    if expr_text.len() < 2 {
        return None;
    }

    // Try to infer the type
    let type_str = infer_expression_type(expr_span, source, parsed, index, uri, func);

    // Find the statement that contains this expression — we'll insert before it
    let insert_offset = find_containing_statement_start(body, expr_span.start)?;
    let (insert_line, _) = line_index.line_col(insert_offset);

    // Figure out indentation of the containing statement
    let indent = get_line_indent(source, insert_offset);

    let var_name = "newVariable";
    let decl_line = format!("{}{} {} = {};\n", indent, type_str, var_name, expr_text);

    let insert_pos = Position::new(insert_line, 0);
    let expr_range = span_to_range(expr_span, line_index);

    let mut edits = vec![
        // Insert the variable declaration before the statement
        TextEdit {
            range: Range::new(insert_pos, insert_pos),
            new_text: decl_line,
        },
        // Replace the expression with the variable name
        TextEdit {
            range: expr_range,
            new_text: var_name.to_string(),
        },
    ];

    // Sort edits in reverse order so byte offsets don't shift
    edits.sort_by(|a, b| b.range.start.cmp(&a.range.start));

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: "Extract to variable".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        command: Some(rename_after_refactor(var_name)),
        ..Default::default()
    })
}

// =============================================================================
// Extract Function
// =============================================================================

/// Produce an "Extract Function" code action if the selection covers one or more statements.
pub fn extract_function(
    _index: &WorkspaceIndex,
    uri: &Url,
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    sel_start: u32,
    sel_end: u32,
) -> Option<CodeAction> {
    let func = find_enclosing_function(parsed, sel_start)?;
    let body = func.body.as_ref()?;

    // Find the innermost block containing the selection (handles else-if, while, etc.)
    let block = find_innermost_block(body, sel_start, sel_end);

    // Find the range of statements that are fully covered by the selection
    let (stmt_indices, stmts_span) = find_selected_statements(block, sel_start, sel_end)?;
    if stmt_indices.is_empty() {
        return None;
    }

    let selected_source = stmts_span.text(source);

    // Collect variables declared BEFORE the selection that are used IN the selection
    // These become parameters of the extracted function.
    let params = find_free_variables(func, block, &stmt_indices, source);

    // Check if the selected code contains a return statement — if so, we need
    // to preserve the return type. For simplicity, only extract void functions
    // unless the selection ends with a return.
    let has_return = selected_statements_have_return(block, &stmt_indices);

    // Determine if any variable declared in the selection is used after it.
    // If so, we'd need to return it — for now, only support void extraction
    // or single-return extraction.
    let return_type = if has_return {
        // Use the enclosing function's return type
        format_type_ref(&func.return_type)
    } else {
        "void".to_string()
    };

    let fn_name = "ExtractedFunction";

    // Build parameter list
    let param_list: String = params
        .iter()
        .map(|p| format!("{} {}", p.type_str, p.name))
        .collect::<Vec<_>>()
        .join(", ");

    let arg_list: String = params
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    // Build the extracted function
    let indent = get_line_indent(source, stmts_span.start);
    let fn_body = reindent(selected_source, &indent, "    ");

    let new_function = format!(
        "{} {}({})\n{{\n{}\n}}\n\n",
        return_type, fn_name, param_list, fn_body
    );

    // Find where to insert the new function (before the enclosing function)
    let (fn_insert_line, _) = line_index.line_col(func.span.start);
    let fn_insert_pos = Position::new(fn_insert_line, 0);

    // Build the replacement call
    let call_expr = if return_type == "void" {
        format!("{}{}({});\n", indent, fn_name, arg_list)
    } else {
        format!("{}return {}({});\n", indent, fn_name, arg_list)
    };

    // Get the range covering the selected statements (full lines)
    let stmts_range = multiline_range(stmts_span, line_index, source);

    let edits = vec![
        // Insert new function before the enclosing function
        TextEdit {
            range: Range::new(fn_insert_pos, fn_insert_pos),
            new_text: new_function,
        },
        // Replace selected statements with the function call
        TextEdit {
            range: stmts_range,
            new_text: call_expr,
        },
    ];

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: "Extract to function".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        command: Some(rename_after_refactor(fn_name)),
        ..Default::default()
    })
}

// =============================================================================
// Extract to File
// =============================================================================

/// Produce an "Extract to file" code action if the cursor is on a function definition.
pub fn extract_to_file(
    uri: &Url,
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    sel_start: u32,
    sel_end: u32,
) -> Option<CodeAction> {
    // Find a function declaration that contains the selection
    let func = parsed.declarations.iter().find_map(|decl| {
        if let Declaration::Function(f) = decl {
            if f.body.is_some()
                && f.span.start <= sel_start
                && sel_end <= f.span.end
            {
                return Some(f);
            }
        }
        None
    })?;

    let func_name = func.name.as_ref()?;

    // Don't offer for entry points
    if matches!(func_name.name.as_str(), "main" | "StartingConditional") {
        return None;
    }

    // Build the new file name from the function name (truncated to 16 chars for NWN)
    let file_stem = truncate_resref(&func_name.name.to_lowercase(), 16);
    let new_file_name = format!("{}.nss", file_stem);

    // Get the file's directory to construct the new file URI
    let file_path = uri.to_file_path().ok()?;
    let dir = file_path.parent()?;
    let new_file_path = dir.join(&new_file_name);
    let new_file_uri = Url::from_file_path(&new_file_path).ok()?;

    // Don't offer if file already exists
    if new_file_path.exists() {
        return None;
    }

    // Extract function text (full lines)
    let func_span = func.span;
    let func_text = func_span.text(source);

    // Also find and extract the prototype if one exists
    let prototype = parsed.declarations.iter().find_map(|decl| {
        if let Declaration::Function(f) = decl {
            if f.body.is_none()
                && f.name.as_ref().map(|n| &n.name) == Some(&func_name.name)
            {
                return Some(f);
            }
        }
        None
    });

    // Collect #include directives from the current file that the extracted
    // function might need (simple heuristic: include all of them)
    let includes: Vec<String> = parsed
        .declarations
        .iter()
        .filter_map(|decl| {
            if let Declaration::Include(inc) = decl {
                inc.path.as_ref().map(|p| format!("#include \"{}\"", p))
            } else {
                None
            }
        })
        .collect();

    // Build the new file content
    let mut new_content = String::new();
    if !includes.is_empty() {
        new_content.push_str(&includes.join("\n"));
        new_content.push_str("\n\n");
    }
    new_content.push_str(func_text);
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    // Build edits for the current file:
    // 1. Remove the function definition (and prototype if exists)
    // 2. Add #include for the new file
    let mut current_file_edits = Vec::new();

    // Remove the function definition
    let removal_range = multiline_range(func_span, line_index, source);
    current_file_edits.push(TextEdit {
        range: removal_range,
        new_text: String::new(),
    });

    // Remove the prototype if it exists
    if let Some(proto) = prototype {
        let proto_range = multiline_range(proto.span, line_index, source);
        current_file_edits.push(TextEdit {
            range: proto_range,
            new_text: String::new(),
        });
    }

    // Add #include for the new file at the import insert position
    let import_pos = find_import_insert_position(parsed, line_index);
    current_file_edits.push(TextEdit {
        range: Range::new(import_pos, import_pos),
        new_text: format!("#include \"{}\"\n", file_stem),
    });

    // Sort edits in reverse order so byte offsets don't shift
    current_file_edits.sort_by(|a, b| {
        b.range
            .start
            .cmp(&a.range.start)
            .then(b.range.end.cmp(&a.range.end))
    });

    // Build document changes: CreateFile + edits for new file + edits for current file
    let operations = vec![
        // 1. Create the new file
        DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
            uri: new_file_uri.clone(),
            options: None,
            annotation_id: None,
        })),
        // 2. Write content to the new file
        DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: new_file_uri.clone(),
                version: None,
            },
            edits: vec![OneOf::Left(TextEdit {
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                new_text: new_content,
            })],
        }),
        // 3. Edit the current file (remove function, add include)
        DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: None,
            },
            edits: current_file_edits.into_iter().map(OneOf::Left).collect(),
        }),
    ];

    Some(CodeAction {
        title: format!("Move \"{}\" to new file \"{}\"", func_name.name, new_file_name),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(operations)),
            ..Default::default()
        }),
        command: Some(Command {
            title: "Rename File".to_string(),
            command: "nwscript-lsp.renameFile".to_string(),
            arguments: Some(vec![serde_json::Value::String(
                new_file_uri.to_string(),
            )]),
        }),
        ..Default::default()
    })
}

// =============================================================================
// AST traversal helpers
// =============================================================================

/// Find the function declaration whose body contains the given offset.
fn find_enclosing_function(parsed: &ParsedFile, offset: u32) -> Option<&FunctionDecl> {
    parsed.declarations.iter().find_map(|decl| {
        if let Declaration::Function(f) = decl {
            if let Some(body) = &f.body {
                if body.span.start <= offset && offset <= body.span.end {
                    return Some(f);
                }
            }
        }
        None
    })
}

/// Find the innermost block that fully contains [sel_start, sel_end].
/// This handles statements inside if/else-if/while/for/switch blocks.
fn find_innermost_block<'a>(block: &'a Block, sel_start: u32, sel_end: u32) -> &'a Block {
    for stmt in &block.stmts {
        if let Some(inner) = find_block_in_stmt(stmt, sel_start, sel_end) {
            return find_innermost_block(inner, sel_start, sel_end);
        }
    }
    block
}

fn find_block_in_stmt<'a>(stmt: &'a Stmt, sel_start: u32, sel_end: u32) -> Option<&'a Block> {
    let span = stmt.span();
    if sel_start < span.start || sel_end > span.end {
        return None;
    }
    match stmt {
        Stmt::Block(b) => {
            if b.span.start <= sel_start && sel_end <= b.span.end {
                Some(b)
            } else {
                None
            }
        }
        Stmt::If(s) => {
            if let Some(b) = find_block_in_stmt(&s.then_branch, sel_start, sel_end) {
                return Some(b);
            }
            if let Some(e) = &s.else_branch {
                return find_block_in_stmt(e, sel_start, sel_end);
            }
            None
        }
        Stmt::While(s) => find_block_in_stmt(&s.body, sel_start, sel_end),
        Stmt::DoWhile(s) => find_block_in_stmt(&s.body, sel_start, sel_end),
        Stmt::For(s) => find_block_in_stmt(&s.body, sel_start, sel_end),
        Stmt::Switch(s) => {
            // Switch cases don't have a Block wrapper, but we can check individual stmts
            for case in &s.cases {
                for cs in &case.stmts {
                    if let Some(b) = find_block_in_stmt(cs, sel_start, sel_end) {
                        return Some(b);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Find the best extractable expression at cursor position.
/// Skips trivial expressions (bare identifiers, simple literals) and walks
/// up to their parent compound expression. For example, cursor on `StringToInt`
/// in `SetLocalInt(oPC, "X", StringToInt(s))` returns the `StringToInt(s)` call.
fn find_extractable_at_cursor(block: &Block, cursor: u32, source: &str) -> Option<Span> {
    let mut candidates: Vec<Span> = Vec::new();
    collect_expr_spans_containing(block, cursor, &mut candidates);

    // Sort by span size ascending — smallest first
    candidates.sort_by_key(|s| s.len());

    // Pick the smallest expression that isn't a bare identifier or simple literal
    candidates.into_iter().find(|span| {
        let text = span.text(source).trim();
        // A bare identifier is all alphanumeric/underscore — skip it
        let is_bare_ident = !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_');
        // A simple numeric literal — skip it
        let is_number = text.parse::<i64>().is_ok()
            || text.parse::<f64>().is_ok()
            || text.starts_with("0x")
            || text.starts_with("0X");
        !is_bare_ident && !is_number
    })
}

/// Collect the spans of all expressions that contain the cursor position.
fn collect_expr_spans_containing(block: &Block, cursor: u32, out: &mut Vec<Span>) {
    for stmt in &block.stmts {
        collect_expr_spans_in_stmt(stmt, cursor, out);
    }
}

fn collect_expr_spans_in_stmt(stmt: &Stmt, cursor: u32, out: &mut Vec<Span>) {
    let span = stmt.span();
    if cursor < span.start || cursor > span.end {
        return;
    }
    match stmt {
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.initializer {
                collect_expr_spans_in_expr(init, cursor, out);
            }
        }
        Stmt::Expr(es) => collect_expr_spans_in_expr(&es.expr, cursor, out),
        Stmt::Return(r) => {
            if let Some(val) = &r.value {
                collect_expr_spans_in_expr(val, cursor, out);
            }
        }
        Stmt::If(s) => {
            collect_expr_spans_in_expr(&s.condition, cursor, out);
            collect_expr_spans_in_stmt(&s.then_branch, cursor, out);
            if let Some(e) = &s.else_branch {
                collect_expr_spans_in_stmt(e, cursor, out);
            }
        }
        Stmt::While(s) => {
            collect_expr_spans_in_expr(&s.condition, cursor, out);
            collect_expr_spans_in_stmt(&s.body, cursor, out);
        }
        Stmt::DoWhile(s) => {
            collect_expr_spans_in_stmt(&s.body, cursor, out);
            collect_expr_spans_in_expr(&s.condition, cursor, out);
        }
        Stmt::For(s) => {
            if let Some(init) = &s.init {
                collect_expr_spans_in_stmt(init, cursor, out);
            }
            if let Some(cond) = &s.condition {
                collect_expr_spans_in_expr(cond, cursor, out);
            }
            if let Some(upd) = &s.update {
                collect_expr_spans_in_expr(upd, cursor, out);
            }
            collect_expr_spans_in_stmt(&s.body, cursor, out);
        }
        Stmt::Switch(s) => {
            collect_expr_spans_in_expr(&s.expr, cursor, out);
            for case in &s.cases {
                for cs in &case.stmts {
                    collect_expr_spans_in_stmt(cs, cursor, out);
                }
            }
        }
        Stmt::Block(b) => collect_expr_spans_containing(b, cursor, out),
        _ => {}
    }
}

fn collect_expr_spans_in_expr(expr: &Expr, cursor: u32, out: &mut Vec<Span>) {
    let span = expr.span();
    if cursor < span.start || cursor > span.end {
        return;
    }
    // Add this expression's span
    out.push(span);

    // Recurse into children
    match expr {
        Expr::Binary(b) => {
            collect_expr_spans_in_expr(&b.left, cursor, out);
            collect_expr_spans_in_expr(&b.right, cursor, out);
        }
        Expr::Unary(u) => collect_expr_spans_in_expr(&u.operand, cursor, out),
        Expr::Postfix(p) => collect_expr_spans_in_expr(&p.operand, cursor, out),
        Expr::Call(c) => {
            collect_expr_spans_in_expr(&c.callee, cursor, out);
            for arg in &c.args {
                collect_expr_spans_in_expr(arg, cursor, out);
            }
        }
        Expr::FieldAccess(f) => collect_expr_spans_in_expr(&f.object, cursor, out),
        Expr::Assignment(a) => {
            collect_expr_spans_in_expr(&a.target, cursor, out);
            collect_expr_spans_in_expr(&a.value, cursor, out);
        }
        Expr::Ternary(t) => {
            collect_expr_spans_in_expr(&t.condition, cursor, out);
            collect_expr_spans_in_expr(&t.then_expr, cursor, out);
            collect_expr_spans_in_expr(&t.else_expr, cursor, out);
        }
        Expr::Paren(inner) => collect_expr_spans_in_expr(inner, cursor, out),
        Expr::VectorLiteral(v) => {
            collect_expr_spans_in_expr(&v.x, cursor, out);
            collect_expr_spans_in_expr(&v.y, cursor, out);
            collect_expr_spans_in_expr(&v.z, cursor, out);
        }
        _ => {}
    }
}

/// Find the smallest expression in the function body that contains [sel_start, sel_end].
fn find_expression_at(block: &Block, sel_start: u32, sel_end: u32) -> Option<Span> {
    let mut best: Option<Span> = None;

    for stmt in &block.stmts {
        if let Some(span) = find_expr_in_stmt(stmt, sel_start, sel_end) {
            if best.is_none() || span.len() < best.unwrap().len() {
                best = Some(span);
            }
        }
    }

    best
}

fn find_expr_in_stmt(stmt: &Stmt, sel_start: u32, sel_end: u32) -> Option<Span> {
    match stmt {
        Stmt::Expr(es) => find_expr_in_expr(&es.expr, sel_start, sel_end),
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.initializer {
                find_expr_in_expr(init, sel_start, sel_end)
            } else {
                None
            }
        }
        Stmt::Return(r) => {
            if let Some(val) = &r.value {
                find_expr_in_expr(val, sel_start, sel_end)
            } else {
                None
            }
        }
        Stmt::If(s) => {
            let mut best = find_expr_in_expr(&s.condition, sel_start, sel_end);
            if let Some(span) = find_expr_in_stmt(&s.then_branch, sel_start, sel_end) {
                if best.is_none() || span.len() < best.unwrap().len() {
                    best = Some(span);
                }
            }
            if let Some(e) = &s.else_branch {
                if let Some(span) = find_expr_in_stmt(e, sel_start, sel_end) {
                    if best.is_none() || span.len() < best.unwrap().len() {
                        best = Some(span);
                    }
                }
            }
            best
        }
        Stmt::While(s) => {
            let mut best = find_expr_in_expr(&s.condition, sel_start, sel_end);
            if let Some(span) = find_expr_in_stmt(&s.body, sel_start, sel_end) {
                if best.is_none() || span.len() < best.unwrap().len() {
                    best = Some(span);
                }
            }
            best
        }
        Stmt::DoWhile(s) => {
            let mut best = find_expr_in_expr(&s.condition, sel_start, sel_end);
            if let Some(span) = find_expr_in_stmt(&s.body, sel_start, sel_end) {
                if best.is_none() || span.len() < best.unwrap().len() {
                    best = Some(span);
                }
            }
            best
        }
        Stmt::For(s) => {
            let mut best: Option<Span> = None;
            if let Some(init) = &s.init {
                best = find_expr_in_stmt(init, sel_start, sel_end);
            }
            if let Some(cond) = &s.condition {
                if let Some(span) = find_expr_in_expr(cond, sel_start, sel_end) {
                    if best.is_none() || span.len() < best.unwrap().len() {
                        best = Some(span);
                    }
                }
            }
            if let Some(upd) = &s.update {
                if let Some(span) = find_expr_in_expr(upd, sel_start, sel_end) {
                    if best.is_none() || span.len() < best.unwrap().len() {
                        best = Some(span);
                    }
                }
            }
            if let Some(span) = find_expr_in_stmt(&s.body, sel_start, sel_end) {
                if best.is_none() || span.len() < best.unwrap().len() {
                    best = Some(span);
                }
            }
            best
        }
        Stmt::Switch(s) => {
            let mut best = find_expr_in_expr(&s.expr, sel_start, sel_end);
            for case in &s.cases {
                for cs in &case.stmts {
                    if let Some(span) = find_expr_in_stmt(cs, sel_start, sel_end) {
                        if best.is_none() || span.len() < best.unwrap().len() {
                            best = Some(span);
                        }
                    }
                }
            }
            best
        }
        Stmt::Block(b) => find_expression_at(b, sel_start, sel_end),
        _ => None,
    }
}

/// Recursively find the smallest expression containing the selection.
fn find_expr_in_expr(expr: &Expr, sel_start: u32, sel_end: u32) -> Option<Span> {
    let span = expr.span();
    if span.start > sel_start || span.end < sel_end {
        return None;
    }

    // Try to find a smaller sub-expression first
    let child = match expr {
        Expr::Binary(b) => {
            let l = find_expr_in_expr(&b.left, sel_start, sel_end);
            let r = find_expr_in_expr(&b.right, sel_start, sel_end);
            pick_smaller(l, r)
        }
        Expr::Unary(u) => find_expr_in_expr(&u.operand, sel_start, sel_end),
        Expr::Postfix(p) => find_expr_in_expr(&p.operand, sel_start, sel_end),
        Expr::Call(c) => {
            let mut best = find_expr_in_expr(&c.callee, sel_start, sel_end);
            for arg in &c.args {
                best = pick_smaller(best, find_expr_in_expr(arg, sel_start, sel_end));
            }
            best
        }
        Expr::FieldAccess(f) => find_expr_in_expr(&f.object, sel_start, sel_end),
        Expr::Assignment(a) => {
            let l = find_expr_in_expr(&a.target, sel_start, sel_end);
            let r = find_expr_in_expr(&a.value, sel_start, sel_end);
            pick_smaller(l, r)
        }
        Expr::Ternary(t) => {
            let c = find_expr_in_expr(&t.condition, sel_start, sel_end);
            let th = find_expr_in_expr(&t.then_expr, sel_start, sel_end);
            let el = find_expr_in_expr(&t.else_expr, sel_start, sel_end);
            pick_smaller(pick_smaller(c, th), el)
        }
        Expr::Paren(inner) => find_expr_in_expr(inner, sel_start, sel_end),
        Expr::VectorLiteral(v) => {
            let x = find_expr_in_expr(&v.x, sel_start, sel_end);
            let y = find_expr_in_expr(&v.y, sel_start, sel_end);
            let z = find_expr_in_expr(&v.z, sel_start, sel_end);
            pick_smaller(pick_smaller(x, y), z)
        }
        _ => None,
    };

    // If we found a smaller child, use it; otherwise use this expression
    Some(child.unwrap_or(span))
}

fn pick_smaller(a: Option<Span>, b: Option<Span>) -> Option<Span> {
    match (a, b) {
        (Some(a), Some(b)) => {
            if a.len() <= b.len() {
                Some(a)
            } else {
                Some(b)
            }
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Find the byte offset of the start of the line containing the statement
/// that encloses the given expression offset.
/// Find the byte offset of the start of the innermost statement containing the
/// given expression offset. Recurses through all compound statement types
/// (if/else-if chains, while, for, switch, nested blocks) to find the deepest
/// block-level statement, so the extracted variable is inserted in the correct scope.
fn find_containing_statement_start(block: &Block, expr_offset: u32) -> Option<u32> {
    for stmt in &block.stmts {
        let span = stmt.span();
        if span.start <= expr_offset && expr_offset <= span.end {
            // Try to find a deeper statement inside this one
            if let Some(inner) = find_deeper_statement(stmt, expr_offset) {
                return Some(inner);
            }
            return Some(span.start);
        }
    }
    None
}

/// Recursively drill into compound statements to find the innermost block-level
/// statement containing the offset. Returns None if this statement has no nested
/// blocks, or the offset isn't inside any of them.
fn find_deeper_statement(stmt: &Stmt, offset: u32) -> Option<u32> {
    match stmt {
        Stmt::Block(b) => find_containing_statement_start(b, offset),
        Stmt::If(s) => {
            if let Some(inner) = find_deeper_statement(&s.then_branch, offset) {
                return Some(inner);
            }
            if let Some(e) = &s.else_branch {
                if let Some(inner) = find_deeper_statement(e, offset) {
                    return Some(inner);
                }
            }
            None
        }
        Stmt::While(s) => find_deeper_statement(&s.body, offset),
        Stmt::DoWhile(s) => find_deeper_statement(&s.body, offset),
        Stmt::For(s) => find_deeper_statement(&s.body, offset),
        Stmt::Switch(s) => {
            for case in &s.cases {
                for cs in &case.stmts {
                    let cs_span = cs.span();
                    if cs_span.start <= offset && offset <= cs_span.end {
                        if let Some(inner) = find_deeper_statement(cs, offset) {
                            return Some(inner);
                        }
                        return Some(cs_span.start);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

// =============================================================================
// Type inference helpers
// =============================================================================

/// Try to infer the type of an expression from context.
fn infer_expression_type(
    expr_span: Span,
    source: &str,
    parsed: &ParsedFile,
    index: &WorkspaceIndex,
    uri: &Url,
    func: &FunctionDecl,
) -> String {
    let expr_text = expr_span.text(source);

    // Parse the expression text to figure out what it is
    // We use the actual AST by finding the expression node
    if let Some(body) = &func.body {
        if let Some(ty) = infer_type_from_ast(body, expr_span, source, parsed, index, uri, func) {
            return ty;
        }
    }

    // Fallback heuristics based on the text
    let trimmed = expr_text.trim();

    // String literal
    if trimmed.starts_with('"') {
        return "string".to_string();
    }

    // Float literal
    if trimmed.contains('.') && trimmed.parse::<f64>().is_ok() {
        return "float".to_string();
    }

    // Int literal
    if trimmed.parse::<i64>().is_ok() {
        return "int".to_string();
    }

    // Hex literal
    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        return "int".to_string();
    }

    // Vector literal
    if trimmed.starts_with('[') || trimmed.starts_with("Vector(") {
        return "vector".to_string();
    }

    "int".to_string()
}

/// Walk the AST to find the expression at the given span and infer its type.
fn infer_type_from_ast(
    block: &Block,
    target_span: Span,
    source: &str,
    parsed: &ParsedFile,
    index: &WorkspaceIndex,
    uri: &Url,
    func: &FunctionDecl,
) -> Option<String> {
    for stmt in &block.stmts {
        if let Some(ty) = infer_type_in_stmt(stmt, target_span, source, parsed, index, uri, func) {
            return Some(ty);
        }
    }
    None
}

fn infer_type_in_stmt(
    stmt: &Stmt,
    target_span: Span,
    source: &str,
    parsed: &ParsedFile,
    index: &WorkspaceIndex,
    uri: &Url,
    func: &FunctionDecl,
) -> Option<String> {
    // Only look in statements that contain the target
    let stmt_span = stmt.span();
    if target_span.start < stmt_span.start || target_span.end > stmt_span.end {
        return None;
    }

    match stmt {
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.initializer {
                if let Some(ty) = infer_type_in_expr(init, target_span, source, parsed, index, uri, func) {
                    return Some(ty);
                }
            }
            None
        }
        Stmt::Expr(es) => infer_type_in_expr(&es.expr, target_span, source, parsed, index, uri, func),
        Stmt::Return(r) => {
            if let Some(val) = &r.value {
                infer_type_in_expr(val, target_span, source, parsed, index, uri, func)
            } else {
                None
            }
        }
        Stmt::If(s) => {
            if let Some(ty) = infer_type_in_expr(&s.condition, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            if let Some(ty) = infer_type_in_stmt(&s.then_branch, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            if let Some(e) = &s.else_branch {
                return infer_type_in_stmt(e, target_span, source, parsed, index, uri, func);
            }
            None
        }
        Stmt::While(s) => {
            if let Some(ty) = infer_type_in_expr(&s.condition, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            infer_type_in_stmt(&s.body, target_span, source, parsed, index, uri, func)
        }
        Stmt::DoWhile(s) => {
            if let Some(ty) = infer_type_in_stmt(&s.body, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            infer_type_in_expr(&s.condition, target_span, source, parsed, index, uri, func)
        }
        Stmt::For(s) => {
            if let Some(init) = &s.init {
                if let Some(ty) = infer_type_in_stmt(init, target_span, source, parsed, index, uri, func) {
                    return Some(ty);
                }
            }
            if let Some(cond) = &s.condition {
                if let Some(ty) = infer_type_in_expr(cond, target_span, source, parsed, index, uri, func) {
                    return Some(ty);
                }
            }
            if let Some(upd) = &s.update {
                if let Some(ty) = infer_type_in_expr(upd, target_span, source, parsed, index, uri, func) {
                    return Some(ty);
                }
            }
            infer_type_in_stmt(&s.body, target_span, source, parsed, index, uri, func)
        }
        Stmt::Switch(s) => {
            if let Some(ty) = infer_type_in_expr(&s.expr, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            for case in &s.cases {
                for cs in &case.stmts {
                    if let Some(ty) = infer_type_in_stmt(cs, target_span, source, parsed, index, uri, func) {
                        return Some(ty);
                    }
                }
            }
            None
        }
        Stmt::Block(b) => infer_type_from_ast(b, target_span, source, parsed, index, uri, func),
        _ => None,
    }
}

fn infer_type_in_expr(
    expr: &Expr,
    target_span: Span,
    source: &str,
    parsed: &ParsedFile,
    index: &WorkspaceIndex,
    uri: &Url,
    func: &FunctionDecl,
) -> Option<String> {
    let span = expr.span();
    if target_span.start < span.start || target_span.end > span.end {
        return None;
    }

    // If this expression exactly matches the target, infer its type
    if span == target_span {
        return type_of_expr(expr, source, parsed, index, uri, func);
    }

    // Recurse into sub-expressions
    match expr {
        Expr::Binary(b) => {
            let l = infer_type_in_expr(&b.left, target_span, source, parsed, index, uri, func);
            if l.is_some() { return l; }
            infer_type_in_expr(&b.right, target_span, source, parsed, index, uri, func)
        }
        Expr::Unary(u) => infer_type_in_expr(&u.operand, target_span, source, parsed, index, uri, func),
        Expr::Postfix(p) => infer_type_in_expr(&p.operand, target_span, source, parsed, index, uri, func),
        Expr::Call(c) => {
            if let Some(ty) = infer_type_in_expr(&c.callee, target_span, source, parsed, index, uri, func) {
                return Some(ty);
            }
            for arg in &c.args {
                if let Some(ty) = infer_type_in_expr(arg, target_span, source, parsed, index, uri, func) {
                    return Some(ty);
                }
            }
            None
        }
        Expr::FieldAccess(f) => infer_type_in_expr(&f.object, target_span, source, parsed, index, uri, func),
        Expr::Assignment(a) => {
            let l = infer_type_in_expr(&a.target, target_span, source, parsed, index, uri, func);
            if l.is_some() { return l; }
            infer_type_in_expr(&a.value, target_span, source, parsed, index, uri, func)
        }
        Expr::Ternary(t) => {
            let c = infer_type_in_expr(&t.condition, target_span, source, parsed, index, uri, func);
            if c.is_some() { return c; }
            let th = infer_type_in_expr(&t.then_expr, target_span, source, parsed, index, uri, func);
            if th.is_some() { return th; }
            infer_type_in_expr(&t.else_expr, target_span, source, parsed, index, uri, func)
        }
        Expr::Paren(inner) => infer_type_in_expr(inner, target_span, source, parsed, index, uri, func),
        Expr::VectorLiteral(v) => {
            let x = infer_type_in_expr(&v.x, target_span, source, parsed, index, uri, func);
            if x.is_some() { return x; }
            let y = infer_type_in_expr(&v.y, target_span, source, parsed, index, uri, func);
            if y.is_some() { return y; }
            infer_type_in_expr(&v.z, target_span, source, parsed, index, uri, func)
        }
        _ => None,
    }
}

/// Determine the type of an expression.
fn type_of_expr(
    expr: &Expr,
    source: &str,
    parsed: &ParsedFile,
    index: &WorkspaceIndex,
    uri: &Url,
    func: &FunctionDecl,
) -> Option<String> {
    match expr {
        Expr::Literal(lit) => match &lit.kind {
            LiteralKind::Int(_) => Some("int".to_string()),
            LiteralKind::Float(_) => Some("float".to_string()),
            LiteralKind::String(_) | LiteralKind::RawString(_) | LiteralKind::HashString(_) => {
                Some("string".to_string())
            }
        },
        Expr::VectorLiteral(_) => Some("vector".to_string()),
        Expr::Ident(ident) => {
            // Look up the identifier in local variables
            if let Some(ty) = find_local_var_type(func, &ident.name) {
                return Some(ty);
            }
            // Look up in workspace symbols
            if let Some(sym) = index.find_symbol(uri, &ident.name) {
                if sym.kind == SymbolKind::Function {
                    return sym.return_type.clone();
                }
                // For variables/constants, parse the type from detail
                return Some(extract_type_from_detail(&sym.detail));
            }
            None
        }
        Expr::Call(call) => {
            // Get the function name and look up its return type
            if let Expr::Ident(callee) = &call.callee {
                if let Some(sym) = index.find_symbol(uri, &callee.name) {
                    return sym.return_type.clone();
                }
            }
            None
        }
        Expr::Binary(b) => {
            // For comparisons, result is always int (bool)
            match b.op {
                BinaryOp::Eq | BinaryOp::Neq | BinaryOp::Lt | BinaryOp::Gt
                | BinaryOp::LtEq | BinaryOp::GtEq | BinaryOp::And | BinaryOp::Or => {
                    Some("int".to_string())
                }
                BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                | BinaryOp::Shl | BinaryOp::Shr => Some("int".to_string()),
                _ => {
                    // For arithmetic, infer from left operand
                    type_of_expr(&b.left, source, parsed, index, uri, func)
                }
            }
        }
        Expr::Unary(u) => match u.op {
            UnaryOp::Not => Some("int".to_string()),
            UnaryOp::BitNot => Some("int".to_string()),
            _ => type_of_expr(&u.operand, source, parsed, index, uri, func),
        },
        Expr::Postfix(p) => type_of_expr(&p.operand, source, parsed, index, uri, func),
        Expr::FieldAccess(_) => {
            // Struct field access — hard to type without full type checking.
            // For vector fields (.x, .y, .z), it's always float.
            None
        }
        Expr::Assignment(a) => type_of_expr(&a.target, source, parsed, index, uri, func),
        Expr::Ternary(t) => type_of_expr(&t.then_expr, source, parsed, index, uri, func),
        Expr::Paren(inner) => type_of_expr(inner, source, parsed, index, uri, func),
        Expr::Error(_) => None,
    }
}

/// Find the type of a local variable or parameter by name within a function.
fn find_local_var_type(func: &FunctionDecl, name: &str) -> Option<String> {
    // Check parameters
    for param in &func.params {
        if let Some(pname) = &param.name {
            if pname.name == name {
                return Some(format_type_ref(&param.ty));
            }
        }
    }

    // Check local variable declarations in the body
    if let Some(body) = &func.body {
        return find_var_type_in_block(body, name);
    }

    None
}

fn find_var_type_in_block(block: &Block, name: &str) -> Option<String> {
    for stmt in &block.stmts {
        if let Some(ty) = find_var_type_in_stmt(stmt, name) {
            return Some(ty);
        }
    }
    None
}

fn find_var_type_in_stmt(stmt: &Stmt, name: &str) -> Option<String> {
    match stmt {
        Stmt::VarDecl(v) => {
            if let Some(vname) = &v.name {
                if vname.name == name {
                    return Some(format_type_ref(&v.ty));
                }
            }
            None
        }
        Stmt::Block(b) => find_var_type_in_block(b, name),
        Stmt::If(s) => {
            let t = find_var_type_in_stmt(&s.then_branch, name);
            if t.is_some() {
                return t;
            }
            if let Some(e) = &s.else_branch {
                return find_var_type_in_stmt(e, name);
            }
            None
        }
        Stmt::While(s) => find_var_type_in_stmt(&s.body, name),
        Stmt::DoWhile(s) => find_var_type_in_stmt(&s.body, name),
        Stmt::For(s) => {
            if let Some(init) = &s.init {
                if let Some(ty) = find_var_type_in_stmt(init, name) {
                    return Some(ty);
                }
            }
            find_var_type_in_stmt(&s.body, name)
        }
        Stmt::Switch(s) => {
            for case in &s.cases {
                for cs in &case.stmts {
                    if let Some(ty) = find_var_type_in_stmt(cs, name) {
                        return Some(ty);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract the type portion from a detail string like "int nCount" or "const string TAG".
fn extract_type_from_detail(detail: &str) -> String {
    let detail = detail.strip_prefix("const ").unwrap_or(detail);
    // The type is everything before the last word
    if let Some(pos) = detail.rfind(' ') {
        detail[..pos].to_string()
    } else {
        detail.to_string()
    }
}

// =============================================================================
// Extract Function helpers
// =============================================================================

struct FreeVariable {
    name: String,
    type_str: String,
}

/// Find the indices and combined span of statements fully within the selection.
fn find_selected_statements(
    block: &Block,
    sel_start: u32,
    sel_end: u32,
) -> Option<(Vec<usize>, Span)> {
    let mut indices = Vec::new();
    let mut combined_span: Option<Span> = None;

    for (i, stmt) in block.stmts.iter().enumerate() {
        let span = stmt.span();
        // Statement must be fully within the selection
        if span.start >= sel_start && span.end <= sel_end {
            indices.push(i);
            combined_span = Some(match combined_span {
                Some(existing) => existing.merge(span),
                None => span,
            });
        }
    }

    if indices.is_empty() {
        return None;
    }

    combined_span.map(|span| (indices, span))
}

/// Find variables that are used in the selected statements but declared before them.
/// These become parameters of the extracted function. Searches all enclosing scopes
/// (function params, all variable declarations in the function body before the selection).
fn find_free_variables(
    func: &FunctionDecl,
    block: &Block,
    stmt_indices: &[usize],
    source: &str,
) -> Vec<FreeVariable> {
    let sel_start = block.stmts[*stmt_indices.first().unwrap()].span().start;

    // Collect ALL variables available at the selection point:
    // 1. Function parameters
    // 2. All local variable declarations in the entire function body before sel_start
    let mut available_vars: Vec<(String, String)> = Vec::new();

    for param in &func.params {
        if let Some(name) = &param.name {
            available_vars.push((name.name.clone(), format_type_ref(&param.ty)));
        }
    }

    if let Some(body) = &func.body {
        collect_vars_before_offset(body, sel_start, &mut available_vars);
    }

    // Collect identifiers used in the selected statements
    let mut used_idents = HashSet::new();
    for &idx in stmt_indices {
        let stmt_span = block.stmts[idx].span();
        let stmt_source = &source[stmt_span.start as usize..stmt_span.end as usize];
        collect_idents_from_source(stmt_source, &mut used_idents);
    }

    // Variables that are both available and used become parameters
    available_vars
        .into_iter()
        .filter(|(name, _)| used_idents.contains(name.as_str()))
        .map(|(name, type_str)| FreeVariable { name, type_str })
        .collect()
}

/// Recursively collect all variable declarations in a block that start before `offset`.
fn collect_vars_before_offset(block: &Block, offset: u32, out: &mut Vec<(String, String)>) {
    for stmt in &block.stmts {
        if stmt.span().start >= offset {
            break;
        }
        collect_vars_before_offset_in_stmt(stmt, offset, out);
    }
}

fn collect_vars_before_offset_in_stmt(stmt: &Stmt, offset: u32, out: &mut Vec<(String, String)>) {
    match stmt {
        Stmt::VarDecl(v) => {
            if v.span.start < offset {
                if let Some(name) = &v.name {
                    out.push((name.name.clone(), format_type_ref(&v.ty)));
                }
            }
        }
        Stmt::Block(b) => collect_vars_before_offset(b, offset, out),
        Stmt::If(s) => {
            collect_vars_before_offset_in_stmt(&s.then_branch, offset, out);
            if let Some(e) = &s.else_branch {
                collect_vars_before_offset_in_stmt(e, offset, out);
            }
        }
        Stmt::While(s) => collect_vars_before_offset_in_stmt(&s.body, offset, out),
        Stmt::DoWhile(s) => collect_vars_before_offset_in_stmt(&s.body, offset, out),
        Stmt::For(s) => {
            if let Some(init) = &s.init {
                collect_vars_before_offset_in_stmt(init, offset, out);
            }
            collect_vars_before_offset_in_stmt(&s.body, offset, out);
        }
        Stmt::Switch(s) => {
            for case in &s.cases {
                for cs in &case.stmts {
                    collect_vars_before_offset_in_stmt(cs, offset, out);
                }
            }
        }
        _ => {}
    }
}

/// Collect identifier-like tokens from source text (simple lexer scan).
fn collect_idents_from_source<'a>(source: &'a str, out: &mut HashSet<&'a str>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.insert(&source[start..i]);
        } else if bytes[i] == b'"' {
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
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
}

/// Check if any of the selected statements contain a return statement.
fn selected_statements_have_return(block: &Block, indices: &[usize]) -> bool {
    for &idx in indices {
        if stmt_has_return(&block.stmts[idx]) {
            return true;
        }
    }
    false
}

fn stmt_has_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::Block(b) => b.stmts.iter().any(|s| stmt_has_return(s)),
        Stmt::If(s) => {
            stmt_has_return(&s.then_branch)
                || s.else_branch.as_ref().map_or(false, |e| stmt_has_return(e))
        }
        Stmt::While(s) => stmt_has_return(&s.body),
        Stmt::DoWhile(s) => stmt_has_return(&s.body),
        Stmt::For(s) => stmt_has_return(&s.body),
        Stmt::Switch(s) => s.cases.iter().any(|c| c.stmts.iter().any(|s| stmt_has_return(s))),
        _ => false,
    }
}

// =============================================================================
// Text manipulation helpers
// =============================================================================

/// Build a Command that triggers rename on a symbol after a refactoring edit.
/// Uses a custom VS Code extension command that finds the symbol in the document
/// and positions the cursor before triggering rename.
fn rename_after_refactor(symbol_name: &str) -> Command {
    Command {
        title: "Rename Symbol".to_string(),
        command: "nwscript-lsp.renameSymbol".to_string(),
        arguments: Some(vec![serde_json::Value::String(symbol_name.to_string())]),
    }
}

fn format_type_ref(ty: &TypeRef) -> String {
    super::symbols::format_type(&ty.kind)
}

/// Get the indentation (whitespace prefix) at a given byte offset's line.
fn get_line_indent(source: &str, offset: u32) -> String {
    let bytes = source.as_bytes();
    // Find start of the line
    let mut line_start = offset as usize;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    // Collect whitespace
    let mut end = line_start;
    while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
        end += 1;
    }
    source[line_start..end].to_string()
}

/// Reindent a block of text: remove the old indent and apply a new one.
fn reindent(text: &str, old_indent: &str, new_indent: &str) -> String {
    text.lines()
        .map(|line| {
            let stripped = line.strip_prefix(old_indent).unwrap_or(line);
            if stripped.is_empty() {
                String::new()
            } else {
                format!("{}{}", new_indent, stripped)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Range covering all lines of a span (start of first line to end of last line + newline).
fn multiline_range(span: Span, line_index: &LineIndex, source: &str) -> Range {
    let (start_line, _) = line_index.line_col(span.start);
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

/// Find insertion position for a new #include (after last existing one).
fn find_import_insert_position(parsed: &ParsedFile, line_index: &LineIndex) -> Position {
    let mut last_include_end: Option<u32> = None;
    for decl in &parsed.declarations {
        if let Declaration::Include(inc) = decl {
            last_include_end = Some(inc.span.end);
        }
    }
    match last_include_end {
        Some(offset) => {
            let (line, _) = line_index.line_col(offset);
            Position::new(line + 1, 0)
        }
        None => Position::new(0, 0),
    }
}

/// Truncate a string to fit within the NWN resref limit.
fn truncate_resref(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        name.to_string()
    } else {
        name[..max_len].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nwscript_parser::parse;

    #[test]
    fn test_get_line_indent() {
        let source = "void main()\n{\n    int x = 5;\n}";
        // "    int x = 5;" starts at offset 14
        assert_eq!(get_line_indent(source, 14), "    ");
    }

    #[test]
    fn test_reindent() {
        let text = "    int x = 5;\n    int y = 10;";
        let result = reindent(text, "    ", "        ");
        assert_eq!(result, "        int x = 5;\n        int y = 10;");
    }

    #[test]
    fn test_find_enclosing_function() {
        let source = "void main()\n{\n    int x = 5;\n}\n";
        let parsed = parse(source);
        // Offset 18 is inside the body
        let func = find_enclosing_function(&parsed, 18);
        assert!(func.is_some());
        assert_eq!(func.unwrap().name.as_ref().unwrap().name, "main");
    }

    #[test]
    fn test_containing_statement_in_else_if() {
        // Reproduces the bug: cursor on StringToInt inside an else-if block
        // should find the statement inside the block, not the top-level if.
        let source = concat!(
            "void main()\n",
            "{\n",
            "    if (x == 1)\n",
            "    {\n",
            "        Foo();\n",
            "    }\n",
            "    else if (x == 2)\n",
            "    {\n",
            "        Bar(StringToInt(s));\n",
            "    }\n",
            "}\n",
        );
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        let cursor = source.find("StringToInt").unwrap() as u32 + 3;
        let stmt_start = find_containing_statement_start(body, cursor);
        assert!(stmt_start.is_some());
        // Should find "Bar(StringToInt(s));", NOT the top-level "if"
        let start = stmt_start.unwrap() as usize;
        assert!(
            source[start..].starts_with("Bar("),
            "Expected statement starting with 'Bar(', got: {:?}",
            &source[start..start + 20.min(source.len() - start)]
        );
    }

    #[test]
    fn test_extract_type_from_detail() {
        assert_eq!(extract_type_from_detail("int nCount"), "int");
        assert_eq!(extract_type_from_detail("const string TAG"), "string");
        assert_eq!(extract_type_from_detail("object oPC"), "object");
    }

    #[test]
    fn test_truncate_resref() {
        assert_eq!(truncate_resref("short", 16), "short");
        assert_eq!(
            truncate_resref("this_is_a_very_long_function_name", 16),
            "this_is_a_very_l"
        );
    }

    #[test]
    fn test_find_expression_at_call() {
        let source = "void main()\n{\n    int x = GetLevel(oPC);\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();
        // Select "GetLevel(oPC)" — byte positions within the initializer
        let init_text = "GetLevel(oPC)";
        let idx = source.find(init_text).unwrap();
        let expr = find_expression_at(body, idx as u32, (idx + init_text.len()) as u32);
        assert!(expr.is_some());
        assert_eq!(expr.unwrap().text(source), init_text);
    }

    #[test]
    fn test_find_selected_statements() {
        let source = "void main()\n{\n    int x = 5;\n    int y = 10;\n    int z = x + y;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Select the first two statements
        let first_start = body.stmts[0].span().start;
        let second_end = body.stmts[1].span().end;
        let result = find_selected_statements(body, first_start, second_end);
        assert!(result.is_some());
        let (indices, _) = result.unwrap();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn test_find_expression_at_literal() {
        let source = "void main()\n{\n    int x = 42 + 10;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Select just "42"
        let idx = source.find("42").unwrap();
        let expr = find_expression_at(body, idx as u32, (idx + 2) as u32);
        assert!(expr.is_some());
        assert_eq!(expr.unwrap().text(source), "42");
    }

    #[test]
    fn test_find_expression_at_binary() {
        let source = "void main()\n{\n    int x = 42 + 10;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Select "42 + 10"
        let start = source.find("42").unwrap();
        let end = source.find("10").unwrap() + 2;
        let expr = find_expression_at(body, start as u32, end as u32);
        assert!(expr.is_some());
        assert_eq!(expr.unwrap().text(source), "42 + 10");
    }

    #[test]
    fn test_type_of_literal() {
        let source = "void main()\n{\n    int x = 42;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Find the literal "42"
        let idx = source.find("42").unwrap();
        let expr_span = find_expression_at(body, idx as u32, (idx + 2) as u32).unwrap();

        // Infer type — without a real index, fallback should catch it
        let ty = infer_expression_type(expr_span, source, &parsed, &stub_index(), &stub_uri(), func);
        assert_eq!(ty, "int");
    }

    #[test]
    fn test_type_of_string_literal() {
        let source = "void main()\n{\n    string s = \"hello\";\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        let idx = source.find("\"hello\"").unwrap();
        let expr_span = find_expression_at(body, idx as u32, (idx + 7) as u32).unwrap();

        let ty = infer_expression_type(expr_span, source, &parsed, &stub_index(), &stub_uri(), func);
        assert_eq!(ty, "string");
    }

    #[test]
    fn test_free_variables_detection() {
        let source = "void Foo(int nParam)\n{\n    int x = 5;\n    int y = x + nParam;\n    int z = y;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 30).unwrap();
        let body = func.body.as_ref().unwrap();

        // Select statement at index 1: "int y = x + nParam;"
        let result = find_selected_statements(
            body,
            body.stmts[1].span().start,
            body.stmts[1].span().end,
        );
        assert!(result.is_some());
        let (indices, _) = result.unwrap();
        assert_eq!(indices, vec![1]);

        let free_vars = find_free_variables(func, body, &indices, source);
        let names: Vec<&str> = free_vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"x"));
        assert!(names.contains(&"nParam"));
        assert!(!names.contains(&"z")); // z is declared after
    }

    #[test]
    fn test_selected_statements_no_return() {
        let source = "void main()\n{\n    int x = 5;\n    int y = 10;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        let (indices, _) = find_selected_statements(
            body,
            body.stmts[0].span().start,
            body.stmts[1].span().end,
        )
        .unwrap();

        assert!(!selected_statements_have_return(body, &indices));
    }

    #[test]
    fn test_selected_statements_with_return() {
        let source = "int Foo()\n{\n    int x = 5;\n    return x;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 15).unwrap();
        let body = func.body.as_ref().unwrap();

        let (indices, _) = find_selected_statements(
            body,
            body.stmts[0].span().start,
            body.stmts[1].span().end,
        )
        .unwrap();

        assert!(selected_statements_have_return(body, &indices));
    }

    #[test]
    fn test_find_local_var_type() {
        let source = "void Foo(string sName)\n{\n    int x = 5;\n    float f = 1.0;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 30).unwrap();

        assert_eq!(find_local_var_type(func, "sName"), Some("string".to_string()));
        assert_eq!(find_local_var_type(func, "x"), Some("int".to_string()));
        assert_eq!(find_local_var_type(func, "f"), Some("float".to_string()));
        assert_eq!(find_local_var_type(func, "nonexistent"), None);
    }

    #[test]
    fn test_cursor_on_function_name_finds_call() {
        // Simulates cursor on "StringToInt" in SetLocalInt(oPC, "X", StringToInt(sArg));
        let source = "void main()\n{\n    SetLocalInt(oPC, \"X\", StringToInt(sArg));\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Place cursor on "StringToInt"
        let cursor = source.find("StringToInt").unwrap() as u32 + 3; // middle of the name
        let expr = find_extractable_at_cursor(body, cursor, source);
        assert!(expr.is_some());
        let text = expr.unwrap().text(source);
        assert_eq!(text, "StringToInt(sArg)");
    }

    #[test]
    fn test_cursor_on_outer_call_finds_whole_call() {
        let source = "void main()\n{\n    SetLocalInt(oPC, \"X\", StringToInt(sArg));\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Place cursor on "SetLocalInt"
        let cursor = source.find("SetLocalInt").unwrap() as u32 + 3;
        let expr = find_extractable_at_cursor(body, cursor, source);
        assert!(expr.is_some());
        let text = expr.unwrap().text(source);
        assert_eq!(text, "SetLocalInt(oPC, \"X\", StringToInt(sArg))");
    }

    #[test]
    fn test_cursor_on_bare_ident_in_binary_skips_to_parent() {
        let source = "void main()\n{\n    int z = x + y;\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Cursor on "x" in "x + y" — bare ident, should skip up to binary expression
        let cursor = source.find("x + y").unwrap() as u32;
        let expr = find_extractable_at_cursor(body, cursor, source);
        assert!(expr.is_some());
        assert_eq!(expr.unwrap().text(source), "x + y");
    }

    #[test]
    fn test_cursor_on_string_literal_is_extractable() {
        let source = "void main()\n{\n    Foo(\"hello world\");\n}\n";
        let parsed = parse(source);
        let func = find_enclosing_function(&parsed, 20).unwrap();
        let body = func.body.as_ref().unwrap();

        // Cursor on string literal — should be extractable (not trivial)
        let cursor = source.find("\"hello world\"").unwrap() as u32 + 2;
        let expr = find_extractable_at_cursor(body, cursor, source);
        assert!(expr.is_some());
        assert_eq!(expr.unwrap().text(source), "\"hello world\"");
    }

    // Stub helpers for tests that need an index/uri but don't do cross-file resolution
    fn stub_index() -> WorkspaceIndex {
        WorkspaceIndex::new(vec![], vec![], vec![])
    }

    fn stub_uri() -> Url {
        Url::parse("file:///test.nss").unwrap()
    }
}
