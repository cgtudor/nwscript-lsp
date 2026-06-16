use crate::ast::*;
use crate::token::{Token, TokenKind};

use super::{BraceStyle, FormatConfig};

/// AST-walking printer that produces formatted NWScript source.
///
/// Walks the AST while maintaining a cursor into the token stream to
/// preserve comments in their correct positions.
pub struct Printer<'a> {
    source: &'a str,
    tokens: &'a [Token],
    parsed: &'a ParsedFile,
    config: &'a FormatConfig,

    // Output state
    output: String,
    indent_level: usize,
    /// Current column (0-based).
    col: usize,
    /// True if we haven't written any non-whitespace on the current line yet.
    at_line_start: bool,

    // Token cursor for comment interleaving.
    // Points to the next unprocessed token.
    cursor: usize,
}

/// A comment collected from the token stream.
struct Comment {
    text: String,
    is_trailing: bool,
    /// Number of blank lines that appeared before this comment.
    blank_lines_before: usize,
}

/// Result of scanning trivia tokens between AST nodes.
struct TriviaResult {
    comments: Vec<Comment>,
    /// Whether any blank lines were found in the scanned region.
    had_blank_lines: bool,
}

/// An include with its associated leading comments.
struct IncludeWithComments {
    path: String,
    leading_comments: Vec<String>,
}

impl<'a> Printer<'a> {
    pub fn new(
        source: &'a str,
        tokens: &'a [Token],
        parsed: &'a ParsedFile,
        config: &'a FormatConfig,
    ) -> Self {
        Self {
            source,
            tokens,
            parsed,
            config,
            output: String::with_capacity(source.len()),
            indent_level: 0,
            col: 0,
            at_line_start: true,
            cursor: 0,
        }
    }

    pub fn print(mut self) -> String {
        self.format_file();
        // Ensure file ends with a single newline
        let trimmed = self.output.trim_end_matches('\n');
        let mut result = trimmed.to_string();
        result.push('\n');
        result
    }

    // =========================================================================
    // Output helpers
    // =========================================================================

    fn write(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.at_line_start {
            let indent = " ".repeat(self.indent_level * self.config.indent_width);
            self.output.push_str(&indent);
            self.col = indent.len();
            self.at_line_start = false;
        }
        self.output.push_str(s);
        if let Some(last_nl) = s.rfind('\n') {
            self.col = s.len() - last_nl - 1;
        } else {
            self.col += s.len();
        }
    }

    fn write_raw(&mut self, s: &str) {
        self.output.push_str(s);
        if let Some(last_nl) = s.rfind('\n') {
            self.col = s.len() - last_nl - 1;
            self.at_line_start = false;
        } else {
            self.col += s.len();
        }
    }

    fn newline(&mut self) {
        self.output.push('\n');
        self.col = 0;
        self.at_line_start = true;
    }

    fn space(&mut self) {
        if self.at_line_start {
            return;
        }
        self.write_raw(" ");
    }

    fn blank_line(&mut self) {
        self.newline();
        self.newline();
    }

    /// Write the current indentation without marking line as started.
    fn write_indent_str(&mut self) {
        let indent = " ".repeat(self.indent_level * self.config.indent_width);
        self.output.push_str(&indent);
        self.col = indent.len();
        self.at_line_start = false;
    }

    /// One extra level of indentation as a string.
    fn continuation_indent_str(&self) -> String {
        " ".repeat((self.indent_level + 1) * self.config.indent_width)
    }

    // =========================================================================
    // Comment handling
    // =========================================================================

    /// Collect comments from the token stream between `cursor` and `target_pos`.
    /// Returns the comments with their classification (trailing vs leading).
    ///
    /// Non-trivia tokens are skipped (the AST formatter handles them).
    /// The cursor always advances forward, keeping pace with the AST walk.
    fn collect_comments_before(&mut self, target_pos: u32) -> TriviaResult {
        let mut comments = Vec::new();
        // If we're at line start in the output (no code written on current line),
        // treat any initial comments as leading, not trailing.
        let mut seen_newline = self.at_line_start;
        let mut blank_count: usize = 0;
        let mut had_blank_lines = false;

        while self.cursor < self.tokens.len() {
            let tok = self.tokens[self.cursor];
            if tok.span.start >= target_pos {
                break;
            }

            match tok.kind {
                TokenKind::Newline => {
                    if seen_newline {
                        blank_count += 1;
                        if blank_count >= 1 {
                            had_blank_lines = true;
                        }
                    }
                    seen_newline = true;
                    self.cursor += 1;
                }
                TokenKind::Whitespace => {
                    self.cursor += 1;
                }
                TokenKind::LineComment | TokenKind::BlockComment => {
                    comments.push(Comment {
                        text: tok.text(self.source).to_string(),
                        is_trailing: !seen_newline,
                        blank_lines_before: if seen_newline { blank_count } else { 0 },
                    });
                    seen_newline = false;
                    blank_count = 0;
                    self.cursor += 1;
                }
                _ => {
                    // Non-trivia token — skip it (handled by AST formatter).
                    // Reset newline/blank tracking since code separates comment groups.
                    seen_newline = false;
                    blank_count = 0;
                    self.cursor += 1;
                }
            }
        }

        TriviaResult {
            comments,
            had_blank_lines,
        }
    }

    /// Emit trailing comments (on same line as previous code).
    fn emit_trailing_comments(&mut self, comments: &[Comment]) {
        for c in comments {
            if c.is_trailing {
                self.write_raw("  ");
                self.write_raw(&c.text);
            }
        }
    }

    /// Emit leading comments (on their own lines before next code).
    /// Handles blank lines between comments respecting `max_blank_lines`.
    fn emit_leading_comments(&mut self, comments: &[Comment]) {
        for comment in comments {
            if comment.is_trailing {
                continue;
            }
            let blanks = comment
                .blank_lines_before
                .min(self.config.max_blank_lines);
            for _ in 0..blanks {
                self.newline();
            }
            self.newline();
            self.write(&comment.text);
        }
    }

    /// Emit leading comments for the very first item in a scope.
    /// Unlike `emit_leading_comments`, does not add a leading newline before the
    /// first comment if we're already at line start.
    fn emit_leading_comments_first(&mut self, comments: &[Comment]) {
        let mut first_leading = true;
        for comment in comments {
            if comment.is_trailing {
                continue;
            }
            if first_leading && self.at_line_start {
                // Don't add extra newline — we're already at line start
                first_leading = false;
            } else {
                let blanks = comment
                    .blank_lines_before
                    .min(self.config.max_blank_lines);
                for _ in 0..blanks {
                    self.newline();
                }
                self.newline();
            }
            self.write(&comment.text);
        }
    }

    /// Advance the cursor past all tokens whose span is within `past_pos`.
    fn advance_cursor_past(&mut self, past_pos: u32) {
        while self.cursor < self.tokens.len() && self.tokens[self.cursor].span.end <= past_pos {
            self.cursor += 1;
        }
    }

    /// Emit any remaining comments/trivia after the last declaration.
    fn emit_trailing_trivia(&mut self) {
        let eof_pos = self.source.len() as u32;
        let trivia = self.collect_comments_before(eof_pos);
        self.emit_trailing_comments(&trivia.comments);
        let has_leading = trivia.comments.iter().any(|c| !c.is_trailing);
        if has_leading {
            self.emit_leading_comments(&trivia.comments);
        }
    }

    // =========================================================================
    // Top-level formatting
    // =========================================================================

    fn format_file(&mut self) {
        if self.parsed.declarations.is_empty() {
            self.emit_trailing_trivia();
            return;
        }

        // Separate includes from other declarations
        let mut includes = Vec::new();
        let mut others = Vec::new();

        for decl in &self.parsed.declarations {
            match decl {
                Declaration::Include(inc) => includes.push(inc),
                _ => others.push(decl),
            }
        }

        // Format includes (with sorting)
        self.format_includes(&includes);

        // Format remaining declarations
        let is_first_decl = includes.is_empty();
        let mut prev_was_lightweight = false;
        for (i, decl) in others.iter().enumerate() {
            let is_first = is_first_decl && i == 0;
            self.format_declaration(decl, is_first, prev_was_lightweight);
            prev_was_lightweight = match decl {
                Declaration::GlobalVar(_) => true,
                Declaration::Function(f) => f.is_prototype(),
                _ => false,
            };
        }

        self.emit_trailing_trivia();
    }

    fn format_includes(&mut self, includes: &[&IncludeDecl]) {
        if includes.is_empty() {
            return;
        }

        // Collect includes with their associated leading comments
        let mut entries: Vec<IncludeWithComments> = Vec::new();

        // Collect any comments before the very first include
        let first_start = includes[0].span.start;
        let pre_trivia = self.collect_comments_before(first_start);
        let mut file_header_comments: Vec<String> = Vec::new();
        for c in &pre_trivia.comments {
            file_header_comments.push(c.text.clone());
        }

        for (i, inc) in includes.iter().enumerate() {
            // Advance cursor past this include's non-trivia tokens
            self.advance_cursor_past(inc.span.end);

            let path = inc.path.clone().unwrap_or_default();

            // Collect comments between this include and the next
            let next_pos = if i + 1 < includes.len() {
                includes[i + 1].span.start
            } else {
                // Find the start of the first non-include declaration or EOF
                let mut end = self.source.len() as u32;
                for decl in &self.parsed.declarations {
                    if !matches!(decl, Declaration::Include(_)) {
                        end = decl.span().start;
                        break;
                    }
                }
                end
            };

            let between_trivia = self.collect_comments_before(next_pos);
            let mut leading_for_next: Vec<String> = Vec::new();
            for c in &between_trivia.comments {
                if c.is_trailing {
                    // Trailing comment on the include line — attach to this include
                    // We'll handle this by appending to the include's output
                    // For simplicity in sorting, store it with the current include
                    leading_for_next.push(c.text.clone());
                } else {
                    leading_for_next.push(c.text.clone());
                }
            }

            entries.push(IncludeWithComments {
                path,
                leading_comments: leading_for_next,
            });
        }

        // Sort includes if configured
        if self.config.sort_includes {
            entries.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
        }

        // Emit file-header comments
        for (i, comment) in file_header_comments.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.write(comment);
        }
        if !file_header_comments.is_empty() {
            self.newline();
        }

        // Emit sorted includes
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 || !file_header_comments.is_empty() {
                self.newline();
            }
            self.write(&format!("#include \"{}\"", entry.path));

            // Emit comments that were between this include and the next
            for comment in &entry.leading_comments {
                self.newline();
                self.write(comment);
            }
        }
    }

    fn format_declaration(
        &mut self,
        decl: &Declaration,
        is_first: bool,
        prev_was_lightweight: bool,
    ) {
        let trivia = self.collect_comments_before(decl.span().start);

        // 1) Emit any trailing comments from the previous declaration
        self.emit_trailing_comments(&trivia.comments);

        // 2) Blank line separator between declarations (not before the first).
        //    "Lightweight" declarations are variable/const declarations and
        //    function prototypes (no body). For consecutive lightweight decls,
        //    only add a blank line if the user had one (preserves grouping).
        //    Function definitions (with bodies) and structs always get a blank line.
        //
        //    When leading comments are present, emit_leading_comments handles
        //    the newline before the comment, so we only need to ensure we're
        //    on a new line (not add a full blank_line on top of it).
        let is_lightweight = match decl {
            Declaration::GlobalVar(_) => true,
            Declaration::Function(f) => f.is_prototype(),
            _ => false,
        };
        let has_leading = trivia.comments.iter().any(|c| !c.is_trailing);
        if !is_first {
            if prev_was_lightweight && is_lightweight {
                // Consecutive lightweight decls: just end the line.
                // emit_leading_comments or the declaration itself starts
                // on the next line. Blank lines are preserved via
                // had_blank_lines in the trivia/comment handling.
                if !has_leading {
                    self.newline();
                    if trivia.had_blank_lines {
                        self.newline();
                    }
                }
                // When has_leading, emit_leading_comments handles spacing.
            } else {
                // Between heavy decls (function defs, structs), always a blank line.
                if !has_leading {
                    self.blank_line();
                }
                // When has_leading, emit_leading_comments handles spacing.
            }
        }

        // 3) Emit leading comments
        if has_leading {
            if is_first {
                self.emit_leading_comments_first(&trivia.comments);
            } else {
                self.emit_leading_comments(&trivia.comments);
            }
            // Newline after last comment, before the declaration
            self.newline();
        }

        // 4) Write the declaration itself
        match decl {
            Declaration::Include(_) => {} // handled in format_includes
            Declaration::Struct(s) => self.format_struct(s),
            Declaration::Function(f) => self.format_function(f),
            Declaration::GlobalVar(v) => self.format_global_var(v),
        }

        self.advance_cursor_past(decl.span().end);
    }

    // =========================================================================
    // Struct formatting
    // =========================================================================

    fn format_struct(&mut self, s: &StructDecl) {
        let name = s
            .name
            .as_ref()
            .map(|n| n.name.as_str())
            .unwrap_or("__unnamed");

        self.write(&format!("struct {name}"));
        self.format_open_brace();
        self.indent_level += 1;

        for (i, field) in s.fields.iter().enumerate() {
            let trivia = self.collect_comments_before(field.span.start);
            self.emit_trailing_comments(&trivia.comments);
            if i == 0 {
                self.emit_leading_comments_first(&trivia.comments);
            } else {
                self.emit_leading_comments(&trivia.comments);
            }
            self.newline();

            let ty = self.format_type(&field.ty);
            let name = field
                .name
                .as_ref()
                .map(|n| n.name.as_str())
                .unwrap_or("__unnamed");
            self.write(&format!("{ty} {name};"));
        }

        self.indent_level -= 1;
        // Comments before closing brace
        let end_pos = s.span.end;
        let trivia = self.collect_comments_before(end_pos);
        self.emit_trailing_comments(&trivia.comments);
        self.emit_leading_comments(&trivia.comments);
        self.newline();
        self.write("};");
    }

    // =========================================================================
    // Function formatting
    // =========================================================================

    fn format_function(&mut self, f: &FunctionDecl) {
        let ret = self.format_type(&f.return_type);
        let name = f
            .name
            .as_ref()
            .map(|n| n.name.as_str())
            .unwrap_or("__unnamed");

        // Format parameters
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| self.format_param(p))
            .collect();

        // Try single-line signature
        let sig_prefix = format!("{ret} {name}(");
        let sig_suffix = if f.is_prototype() { ");" } else { ")" };
        let params_inline = params.join(if self.config.space_after_comma {
            ", "
        } else {
            ","
        });
        let one_line = format!("{sig_prefix}{params_inline}{sig_suffix}");
        let base_indent_width = self.indent_level * self.config.indent_width;

        if base_indent_width + one_line.len() <= self.config.max_line_width || params.is_empty() {
            self.write(&one_line);
        } else {
            // Wrap parameters
            let cont = self.continuation_indent_str();
            let sep = if self.config.space_after_comma {
                format!(",\n{cont}")
            } else {
                format!(",\n{cont}")
            };
            self.write(&sig_prefix);
            self.newline();
            self.write_indent_str();
            self.write_raw(&cont);
            self.write_raw(&params.join(&sep));
            self.write_raw(sig_suffix);
        }

        if let Some(body) = &f.body {
            self.format_open_brace();
            self.format_block_body(body);
            self.newline();
            self.write("}");
        }
    }

    fn format_param(&self, p: &Param) -> String {
        let ty = self.format_type(&p.ty);
        let name = p
            .name
            .as_ref()
            .map(|n| n.name.as_str())
            .unwrap_or("__unnamed");

        match &p.default_value {
            Some(default) => {
                let val = self.format_expr_str(default, 0);
                if self.config.space_around_operators {
                    format!("{ty} {name} = {val}")
                } else {
                    format!("{ty} {name}={val}")
                }
            }
            None => format!("{ty} {name}"),
        }
    }

    // =========================================================================
    // Block and statement formatting
    // =========================================================================

    fn format_open_brace(&mut self) {
        match self.config.brace_style {
            BraceStyle::NextLine => {
                self.newline();
                self.write("{");
            }
            BraceStyle::SameLine => {
                self.space();
                self.write_raw("{");
            }
        }
    }

    fn format_block_body(&mut self, block: &Block) {
        self.indent_level += 1;

        for (i, stmt) in block.stmts.iter().enumerate() {
            let trivia = self.collect_comments_before(stmt.span().start);
            let is_first = i == 0;

            // Emit trailing comments from previous statement
            self.emit_trailing_comments(&trivia.comments);

            // C#-style: preserve blank lines between statements for logical grouping.
            // When leading comments are present, emit_leading_comments handles
            // the blank line via the comment's own blank_lines_before tracking.
            let has_leading = trivia.comments.iter().any(|c| !c.is_trailing);
            if !is_first && trivia.had_blank_lines && !has_leading {
                self.newline(); // extra blank line
            }

            // Emit leading comments
            if has_leading {
                self.emit_leading_comments(&trivia.comments);
            }

            self.format_stmt(stmt);
        }

        // Emit any trailing comments/trivia before the closing brace
        let block_end = block.span.end;
        let end_trivia = self.collect_comments_before(block_end);
        self.emit_trailing_comments(&end_trivia.comments);
        let has_leading_end = end_trivia.comments.iter().any(|c| !c.is_trailing);
        if has_leading_end {
            self.emit_leading_comments(&end_trivia.comments);
        }

        self.indent_level -= 1;
    }

    fn format_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(v) => {
                self.newline();
                self.format_var_decl_stmt(v);
            }
            Stmt::Expr(e) => {
                self.newline();
                let expr_str = self.format_expr_str(&e.expr, self.current_indent_width());
                self.write(&expr_str);
                self.write_raw(";");
            }
            Stmt::If(s) => {
                self.newline();
                self.format_if(s);
            }
            Stmt::While(s) => {
                self.newline();
                self.format_while(s);
            }
            Stmt::DoWhile(s) => {
                self.newline();
                self.format_do_while(s);
            }
            Stmt::For(s) => {
                self.newline();
                self.format_for(s);
            }
            Stmt::Switch(s) => {
                self.newline();
                self.format_switch(s);
            }
            Stmt::Return(r) => {
                self.newline();
                self.format_return(r);
            }
            Stmt::Break(_) => {
                self.newline();
                self.write("break;");
            }
            Stmt::Continue(_) => {
                self.newline();
                self.write("continue;");
            }
            Stmt::Block(b) => {
                self.format_open_brace();
                self.format_block_body(b);
                self.newline();
                self.write("}");
            }
            Stmt::Empty(_) => {
                self.newline();
                self.write(";");
            }
        }
        self.advance_cursor_past(stmt.span().end);
    }

    fn format_var_decl_stmt(&mut self, v: &VarDecl) {
        let mut line = String::new();
        if v.is_const {
            line.push_str("const ");
        }
        let ty = self.format_type(&v.ty);
        let name = v
            .name
            .as_ref()
            .map(|n| n.name.as_str())
            .unwrap_or("__unnamed");

        match &v.initializer {
            Some(init) => {
                let assign = if self.config.space_around_operators {
                    " = "
                } else {
                    "="
                };
                line.push_str(&format!("{ty} {name}{assign}"));
                let base_col = self.current_indent_width() + line.len();
                let val = self.format_expr_str(init, base_col);
                line.push_str(&val);
                line.push(';');
            }
            None => {
                line.push_str(&format!("{ty} {name};"));
            }
        }
        self.write(&line);
    }

    fn format_global_var(&mut self, v: &VarDecl) {
        self.format_var_decl_stmt(v);
    }

    // =========================================================================
    // Control flow formatting
    // =========================================================================

    fn format_if(&mut self, s: &IfStmt) {
        let kw_space = if self.config.space_after_keywords {
            " "
        } else {
            ""
        };
        let cond = self.format_expr_str(&s.condition, self.current_indent_width());

        if self.config.space_inside_parens {
            self.write(&format!("if{kw_space}( {cond} )"));
        } else {
            self.write(&format!("if{kw_space}({cond})"));
        }

        self.format_stmt_body(&s.then_branch);

        if let Some(else_branch) = &s.else_branch {
            // Handle `else if` chains
            match else_branch.as_ref() {
                Stmt::If(else_if) => {
                    self.newline();
                    self.write("else");

                    let else_kw_space = if self.config.space_after_keywords {
                        " "
                    } else {
                        ""
                    };
                    let econd = self.format_expr_str(&else_if.condition, self.current_indent_width());

                    if self.config.space_inside_parens {
                        self.write_raw(&format!(" if{else_kw_space}( {econd} )"));
                    } else {
                        self.write_raw(&format!(" if{else_kw_space}({econd})"));
                    }

                    self.format_stmt_body(&else_if.then_branch);

                    if let Some(nested_else) = &else_if.else_branch {
                        self.format_else_branch(nested_else);
                    }
                }
                _ => {
                    self.format_else_branch(else_branch);
                }
            }
        }
    }

    fn format_else_branch(&mut self, else_branch: &Stmt) {
        match else_branch {
            Stmt::If(else_if) => {
                self.newline();
                self.write("else");

                let kw_space = if self.config.space_after_keywords {
                    " "
                } else {
                    ""
                };
                let cond = self.format_expr_str(&else_if.condition, self.current_indent_width());

                if self.config.space_inside_parens {
                    self.write_raw(&format!(" if{kw_space}( {cond} )"));
                } else {
                    self.write_raw(&format!(" if{kw_space}({cond})"));
                }

                self.format_stmt_body(&else_if.then_branch);

                if let Some(nested) = &else_if.else_branch {
                    self.format_else_branch(nested);
                }
            }
            _ => {
                self.newline();
                self.write("else");
                self.format_stmt_body(else_branch);
            }
        }
    }

    /// Format the body of a control flow statement.
    /// If it's a block, format with braces. Otherwise, wrap in braces (enforce braces).
    /// Format a statement body, wrapping in braces if needed.
    ///
    /// For braceless statements (e.g., `if (x) stmt;`), wraps in braces.
    /// Any trailing comment on the inline statement (e.g., `// Maul`) is
    /// emitted as a comment above the statement inside the block.
    fn format_stmt_body(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(b) => {
                self.format_open_brace();
                self.format_block_body(b);
                self.newline();
                self.write("}");
            }
            _ => {
                // Always wrap in braces (enforce C# style).
                // Peek ahead: if the inline statement has a trailing comment,
                // emit it as a leading comment inside the block.
                let end_pos = stmt.span().end;
                let saved_cursor = self.cursor;
                let trivia = self.collect_comments_before(end_pos + 100);
                let trailing_comment = trivia
                    .comments
                    .iter()
                    .find(|c| c.is_trailing)
                    .map(|c| c.text.clone());
                // If no trailing comment found, restore cursor so normal
                // processing handles it.
                if trailing_comment.is_none() {
                    self.cursor = saved_cursor;
                }

                self.format_open_brace();
                self.indent_level += 1;

                if let Some(comment) = &trailing_comment {
                    self.newline();
                    self.write(comment);
                }

                self.format_stmt(stmt);
                self.indent_level -= 1;
                self.newline();
                self.write("}");
            }
        }
    }

    fn format_while(&mut self, s: &WhileStmt) {
        let kw_space = if self.config.space_after_keywords {
            " "
        } else {
            ""
        };
        let cond = self.format_expr_str(&s.condition, self.current_indent_width());

        if self.config.space_inside_parens {
            self.write(&format!("while{kw_space}( {cond} )"));
        } else {
            self.write(&format!("while{kw_space}({cond})"));
        }

        self.format_stmt_body(&s.body);
    }

    fn format_do_while(&mut self, s: &DoWhileStmt) {
        self.write("do");
        self.format_stmt_body(&s.body);

        let kw_space = if self.config.space_after_keywords {
            " "
        } else {
            ""
        };
        let cond = self.format_expr_str(&s.condition, self.current_indent_width());

        self.newline();
        if self.config.space_inside_parens {
            self.write(&format!("while{kw_space}( {cond} );"));
        } else {
            self.write(&format!("while{kw_space}({cond});"));
        }
    }

    fn format_for(&mut self, s: &ForStmt) {
        let kw_space = if self.config.space_after_keywords {
            " "
        } else {
            ""
        };

        let init = match &s.init {
            Some(init) => self.format_for_init(init),
            None => String::new(),
        };
        let cond = match &s.condition {
            Some(c) => self.format_expr_str(c, self.current_indent_width()),
            None => String::new(),
        };
        let update = match &s.update {
            Some(u) => self.format_expr_str(u, self.current_indent_width()),
            None => String::new(),
        };

        let sep = if self.config.space_after_comma {
            "; "
        } else {
            ";"
        };

        if self.config.space_inside_parens {
            self.write(&format!("for{kw_space}( {init}{sep}{cond}{sep}{update} )"));
        } else {
            self.write(&format!("for{kw_space}({init}{sep}{cond}{sep}{update})"));
        }

        self.format_stmt_body(&s.body);
    }

    fn format_for_init(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::VarDecl(v) => {
                let ty = self.format_type(&v.ty);
                let name = v
                    .name
                    .as_ref()
                    .map(|n| n.name.as_str())
                    .unwrap_or("__unnamed");
                match &v.initializer {
                    Some(init) => {
                        let val = self.format_expr_str(init, 0);
                        if self.config.space_around_operators {
                            format!("{ty} {name} = {val}")
                        } else {
                            format!("{ty} {name}={val}")
                        }
                    }
                    None => format!("{ty} {name}"),
                }
            }
            Stmt::Expr(e) => self.format_expr_str(&e.expr, 0),
            _ => String::new(),
        }
    }

    fn format_switch(&mut self, s: &SwitchStmt) {
        let kw_space = if self.config.space_after_keywords {
            " "
        } else {
            ""
        };
        let expr = self.format_expr_str(&s.expr, self.current_indent_width());

        if self.config.space_inside_parens {
            self.write(&format!("switch{kw_space}( {expr} )"));
        } else {
            self.write(&format!("switch{kw_space}({expr})"));
        }

        self.format_open_brace();
        self.indent_level += 1;

        for case in &s.cases {
            let trivia = self.collect_comments_before(case.span.start);
            self.emit_trailing_comments(&trivia.comments);
            self.emit_leading_comments(&trivia.comments);

            self.newline();
            match &case.label {
                CaseLabel::Case(expr) => {
                    let val = self.format_expr_str(expr, self.current_indent_width());
                    self.write(&format!("case {val}:"));
                }
                CaseLabel::Default => {
                    self.write("default:");
                }
            }

            self.indent_level += 1;
            for stmt in &case.stmts {
                let trivia = self.collect_comments_before(stmt.span().start);
                self.emit_trailing_comments(&trivia.comments);
                self.emit_leading_comments(&trivia.comments);
                self.format_stmt(stmt);
            }
            self.indent_level -= 1;

            self.advance_cursor_past(case.span.end);
        }

        self.indent_level -= 1;
        self.newline();
        self.write("}");
    }

    fn format_return(&mut self, r: &ReturnStmt) {
        match &r.value {
            Some(val) => {
                let expr = self.format_expr_str(val, self.current_indent_width() + 7); // "return "
                self.write(&format!("return {expr};"));
            }
            None => {
                self.write("return;");
            }
        }
    }

    // =========================================================================
    // Expression formatting (returns string, does not write to output)
    // =========================================================================

    fn format_expr_str(&self, expr: &Expr, base_col: usize) -> String {
        let cont = self.continuation_indent_str();
        self.format_expr_inner(expr, base_col, &cont)
    }

    /// Format an expression to a string.
    ///
    /// `base_col` is the column where the expression begins, used for line-width
    /// decisions. `cont` is the indentation string used for any continuation
    /// lines produced when the expression wraps (call arguments, binary chains).
    fn format_expr_inner(&self, expr: &Expr, base_col: usize, cont: &str) -> String {
        match expr {
            Expr::Literal(lit) => lit.span.text(self.source).to_string(),

            Expr::Ident(id) => id.name.clone(),

            Expr::Binary(bin) => self.format_binary(bin, base_col, cont),

            Expr::Unary(un) => {
                let op = format_unary_op(un.op);
                let operand = self.format_expr_inner(&un.operand, base_col + op.len(), cont);
                format!("{op}{operand}")
            }

            Expr::Postfix(pf) => {
                let operand = self.format_expr_inner(&pf.operand, base_col, cont);
                let op = match pf.op {
                    PostfixOp::Inc => "++",
                    PostfixOp::Dec => "--",
                };
                format!("{operand}{op}")
            }

            Expr::Call(call) => self.format_call(call, base_col, cont),

            Expr::FieldAccess(fa) => {
                let obj = self.format_expr_inner(&fa.object, base_col, cont);
                format!("{obj}.{}", fa.field.name)
            }

            Expr::Assignment(a) => {
                let target = self.format_expr_inner(&a.target, base_col, cont);
                let op = format_assign_op(a.op);
                let value_col = if self.config.space_around_operators {
                    base_col + target.len() + op.len() + 2
                } else {
                    base_col + target.len() + op.len()
                };
                let value = self.format_expr_inner(&a.value, value_col, cont);
                if self.config.space_around_operators {
                    format!("{target} {op} {value}")
                } else {
                    format!("{target}{op}{value}")
                }
            }

            Expr::Ternary(t) => {
                let cond = self.format_expr_inner(&t.condition, base_col, cont);
                let then_e = self.format_expr_inner(&t.then_expr, base_col, cont);
                let else_e = self.format_expr_inner(&t.else_expr, base_col, cont);
                if self.config.space_around_operators {
                    format!("{cond} ? {then_e} : {else_e}")
                } else {
                    format!("{cond}?{then_e}:{else_e}")
                }
            }

            Expr::Paren(inner) => {
                let inner_str = self.format_expr_inner(inner, base_col + 1, cont);
                if self.config.space_inside_parens {
                    format!("( {inner_str} )")
                } else {
                    format!("({inner_str})")
                }
            }

            Expr::VectorLiteral(v) => {
                let x = self.format_expr_inner(&v.x, base_col, cont);
                let y = self.format_expr_inner(&v.y, base_col, cont);
                let z = self.format_expr_inner(&v.z, base_col, cont);
                if self.config.space_after_comma {
                    format!("[{x}, {y}, {z}]")
                } else {
                    format!("[{x},{y},{z}]")
                }
            }

            Expr::Error(span) => {
                // Preserve original text for error nodes
                span.text(self.source).to_string()
            }
        }
    }

    /// Format a function call, wrapping arguments one-per-line (Prettier-style)
    /// when the single-line form would exceed `max_line_width`.
    fn format_call(&self, call: &CallExpr, base_col: usize, cont: &str) -> String {
        let callee = self.format_expr_inner(&call.callee, base_col, cont);
        if call.args.is_empty() {
            return format!("{callee}()");
        }

        let sep = if self.config.space_after_comma {
            ", "
        } else {
            ","
        };
        let args_inline: Vec<String> = call
            .args
            .iter()
            .map(|a| self.format_expr_inner(a, base_col, cont))
            .collect();
        let joined = args_inline.join(sep);

        let inner = if self.config.space_inside_parens {
            format!("( {joined} )")
        } else {
            format!("({joined})")
        };
        let one_line = format!("{callee}{inner}");

        if base_col + one_line.len() <= self.config.max_line_width {
            return one_line;
        }

        // Wrap arguments — each on its own line at the continuation indent.
        // Re-format each argument at its true column so nested constructs (e.g.
        // long string concatenations) wrap relative to where they actually sit.
        let indent_unit = " ".repeat(self.config.indent_width);
        let arg_cont = format!("{cont}{indent_unit}");
        let args: Vec<String> = call
            .args
            .iter()
            .map(|a| self.format_expr_inner(a, cont.len(), &arg_cont))
            .collect();
        let wrap_sep = format!(",\n{cont}");
        let args_wrapped = args.join(&wrap_sep);
        format!("{callee}(\n{cont}{args_wrapped})")
    }

    /// Format a binary expression. Same-precedence operator chains are flattened
    /// and, when the flat form exceeds `max_line_width`, broken one-operand-per-line
    /// with the operator trailing each line (Prettier-style). String literals are
    /// never split, so a single over-long literal simply overflows.
    fn format_binary(&self, bin: &BinaryExpr, base_col: usize, cont: &str) -> String {
        let mut operands: Vec<&Expr> = Vec::new();
        let mut ops: Vec<BinaryOp> = Vec::new();
        self.flatten_binary(bin, &mut operands, &mut ops);

        // Build the flat single-line form.
        let parts: Vec<String> = operands
            .iter()
            .map(|o| self.format_expr_inner(o, base_col, cont))
            .collect();
        let mut flat = String::new();
        for (i, part) in parts.iter().enumerate() {
            flat.push_str(part);
            if i < ops.len() {
                if self.config.space_around_operators {
                    flat.push_str(&format!(" {} ", format_binary_op(ops[i])));
                } else {
                    flat.push_str(format_binary_op(ops[i]));
                }
            }
        }

        if base_col + flat.len() <= self.config.max_line_width {
            return flat;
        }

        // Break: operator trailing each line, continuation lines indented to `cont`.
        let indent_unit = " ".repeat(self.config.indent_width);
        let child_cont = format!("{cont}{indent_unit}");
        let mut result = String::new();
        for (i, operand) in operands.iter().enumerate() {
            let oc = if i == 0 { base_col } else { cont.len() };
            let s = self.format_expr_inner(operand, oc, &child_cont);
            if i > 0 {
                result.push_str(cont);
            }
            result.push_str(&s);
            if i < ops.len() {
                if self.config.space_around_operators {
                    result.push(' ');
                }
                result.push_str(format_binary_op(ops[i]));
                result.push('\n');
            }
        }
        result
    }

    /// Flatten a left-associative chain of same-precedence binary operators into
    /// flat lists of operands and operators.
    fn flatten_binary<'e>(
        &self,
        bin: &'e BinaryExpr,
        operands: &mut Vec<&'e Expr>,
        ops: &mut Vec<BinaryOp>,
    ) {
        match &bin.left {
            Expr::Binary(left_bin)
                if binary_prec_group(left_bin.op) == binary_prec_group(bin.op) =>
            {
                self.flatten_binary(left_bin, operands, ops);
            }
            _ => operands.push(&bin.left),
        }
        ops.push(bin.op);
        operands.push(&bin.right);
    }

    // =========================================================================
    // Type formatting
    // =========================================================================

    fn format_type(&self, ty: &TypeRef) -> String {
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
            TypeKind::Error => ty.span.text(self.source).to_string(),
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn current_indent_width(&self) -> usize {
        self.indent_level * self.config.indent_width
    }
}

// =============================================================================
// Operator formatting helpers (free functions)
// =============================================================================

fn format_binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Eq => "==",
        BinaryOp::Neq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::LtEq => "<=",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
    }
}

/// Precedence-group key for binary operators. Operators in the same group are
/// flattened together when wrapping a long binary chain (e.g. `+` and `-`). Only
/// equality between keys is meaningful; the absolute values are arbitrary.
fn binary_prec_group(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::BitOr => 3,
        BinaryOp::BitXor => 4,
        BinaryOp::BitAnd => 5,
        BinaryOp::Eq | BinaryOp::Neq => 6,
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => 7,
        BinaryOp::Shl | BinaryOp::Shr => 8,
        BinaryOp::Add | BinaryOp::Sub => 9,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 10,
    }
}

fn format_unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
        UnaryOp::PreInc => "++",
        UnaryOp::PreDec => "--",
    }
}

fn format_assign_op(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::AddAssign => "+=",
        AssignOp::SubAssign => "-=",
        AssignOp::MulAssign => "*=",
        AssignOp::DivAssign => "/=",
        AssignOp::ModAssign => "%=",
    }
}
