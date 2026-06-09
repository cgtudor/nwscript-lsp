use nwscript_parser::ast::*;
use nwscript_parser::{LineIndex, ParsedFile};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Url};

use crate::index::WorkspaceIndex;

/// Compute parameter name inlay hints for a file.
pub fn inlay_hints(
    parsed: &ParsedFile,
    line_index: &LineIndex,
    index: &WorkspaceIndex,
    uri: &Url,
    suppress_single_arg: bool,
) -> Vec<InlayHint> {
    let mut collector = HintCollector {
        line_index,
        index,
        uri,
        suppress_single_arg,
        hints: Vec::new(),
    };

    for decl in &parsed.declarations {
        collector.visit_declaration(decl);
    }

    collector.hints
}

struct HintCollector<'a> {
    line_index: &'a LineIndex,
    index: &'a WorkspaceIndex,
    uri: &'a Url,
    suppress_single_arg: bool,
    hints: Vec<InlayHint>,
}

impl<'a> HintCollector<'a> {
    fn visit_declaration(&mut self, decl: &Declaration) {
        match decl {
            Declaration::Function(f) => self.visit_function(f),
            Declaration::GlobalVar(v) => {
                if let Some(init) = &v.initializer {
                    self.visit_expr(init);
                }
            }
            _ => {}
        }
    }

    fn visit_function(&mut self, f: &FunctionDecl) {
        // Visit default parameter values
        for param in &f.params {
            if let Some(default) = &param.default_value {
                self.visit_expr(default);
            }
        }

        if let Some(body) = &f.body {
            self.visit_block(body);
        }
    }

    fn visit_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.initializer {
                    self.visit_expr(init);
                }
            }
            Stmt::Expr(e) => self.visit_expr(&e.expr),
            Stmt::If(s) => {
                self.visit_expr(&s.condition);
                self.visit_stmt(&s.then_branch);
                if let Some(else_branch) = &s.else_branch {
                    self.visit_stmt(else_branch);
                }
            }
            Stmt::While(s) => {
                self.visit_expr(&s.condition);
                self.visit_stmt(&s.body);
            }
            Stmt::DoWhile(s) => {
                self.visit_stmt(&s.body);
                self.visit_expr(&s.condition);
            }
            Stmt::For(s) => {
                if let Some(init) = &s.init {
                    self.visit_stmt(init);
                }
                if let Some(cond) = &s.condition {
                    self.visit_expr(cond);
                }
                if let Some(update) = &s.update {
                    self.visit_expr(update);
                }
                self.visit_stmt(&s.body);
            }
            Stmt::Switch(s) => {
                self.visit_expr(&s.expr);
                for case in &s.cases {
                    if let CaseLabel::Case(expr) = &case.label {
                        self.visit_expr(expr);
                    }
                    for case_stmt in &case.stmts {
                        self.visit_stmt(case_stmt);
                    }
                }
            }
            Stmt::Return(s) => {
                if let Some(val) = &s.value {
                    self.visit_expr(val);
                }
            }
            Stmt::Block(b) => self.visit_block(b),
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call(c) => {
                self.emit_call_hints(c);
                // Recurse into callee (for chained calls) and args (for nested calls)
                self.visit_expr(&c.callee);
                for arg in &c.args {
                    self.visit_expr(arg);
                }
            }
            Expr::Binary(b) => {
                self.visit_expr(&b.left);
                self.visit_expr(&b.right);
            }
            Expr::Unary(u) => {
                self.visit_expr(&u.operand);
            }
            Expr::Postfix(p) => {
                self.visit_expr(&p.operand);
            }
            Expr::FieldAccess(f) => {
                self.visit_expr(&f.object);
            }
            Expr::Assignment(a) => {
                self.visit_expr(&a.target);
                self.visit_expr(&a.value);
            }
            Expr::Ternary(t) => {
                self.visit_expr(&t.condition);
                self.visit_expr(&t.then_expr);
                self.visit_expr(&t.else_expr);
            }
            Expr::Paren(e) => self.visit_expr(e),
            Expr::VectorLiteral(v) => {
                self.visit_expr(&v.x);
                self.visit_expr(&v.y);
                self.visit_expr(&v.z);
            }
            Expr::Literal(_) | Expr::Ident(_) | Expr::Error(_) => {}
        }
    }

    fn emit_call_hints(&mut self, call: &CallExpr) {
        // Only handle simple identifier callees (not method calls)
        let callee_name = match &call.callee {
            Expr::Ident(ident) => &ident.name,
            _ => return,
        };

        // Look up the function in the index
        let Some(sym) = self.index.find_symbol(self.uri, callee_name) else {
            return;
        };
        let Some(params) = &sym.params else {
            return;
        };

        if call.args.is_empty() {
            return;
        }

        if self.suppress_single_arg && call.args.len() == 1 {
            return;
        }

        for (i, arg) in call.args.iter().enumerate() {
            let Some(param) = params.get(i) else {
                break;
            };

            if param.name.is_empty() {
                continue;
            }

            // Suppress hint when the argument is an identifier matching the param name
            if let Expr::Ident(ident) = arg {
                if ident.name.eq_ignore_ascii_case(&param.name) {
                    continue;
                }
            }

            let (line, col) = self.line_index.line_col(arg.span().start);
            self.hints.push(InlayHint {
                position: Position::new(line, col),
                label: InlayHintLabel::String(format!("{}:", param.name)),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::WorkspaceIndex;

    /// Extract the label text from an InlayHint (InlayHintLabel has no PartialEq).
    fn label_text(hint: &InlayHint) -> &str {
        match &hint.label {
            InlayHintLabel::String(s) => s.as_str(),
            InlayHintLabel::LabelParts(parts) => {
                if parts.len() == 1 {
                    &parts[0].value
                } else {
                    panic!("expected single label part, got {}", parts.len())
                }
            }
        }
    }

    /// Helper: build an index with one file containing the given source,
    /// then compute inlay hints for a second file that includes it.
    fn hints_for(lib_source: &str, file_source: &str) -> Vec<InlayHint> {
        let index = WorkspaceIndex::new(vec![], vec![], vec![]);

        // Index the library file (defines functions)
        let lib_uri = Url::parse("file:///lib.nss").unwrap();
        index.update_file(&lib_uri, lib_source.to_string());

        // Index the file under test (which includes the lib)
        let file_uri = Url::parse("file:///test.nss").unwrap();
        index.update_file(&file_uri, file_source.to_string());

        let parsed = nwscript_parser::parse(file_source);
        let line_index = LineIndex::new(file_source);

        inlay_hints(&parsed, &line_index, &index, &file_uri, false)
    }

    #[test]
    fn no_args_no_hints() {
        let hints = hints_for("void Foo() {}", "#include \"lib\"\nvoid main() { Foo(); }");
        assert!(hints.is_empty());
    }

    #[test]
    fn single_arg_hint() {
        let hints = hints_for(
            "void Foo(int nCount) {}",
            "#include \"lib\"\nvoid main() { Foo(42); }",
        );
        assert_eq!(hints.len(), 1);
        assert_eq!(label_text(&hints[0]), "nCount:");
        assert_eq!(hints[0].kind, Some(InlayHintKind::PARAMETER));
    }

    #[test]
    fn multiple_args() {
        let hints = hints_for(
            "void Bar(int nType, string sName, object oTarget) {}",
            "#include \"lib\"\nvoid main() { Bar(1, \"hello\", OBJECT_SELF); }",
        );
        assert_eq!(hints.len(), 3);
        assert_eq!(label_text(&hints[0]), "nType:");
        assert_eq!(label_text(&hints[1]), "sName:");
        assert_eq!(label_text(&hints[2]), "oTarget:");
    }

    #[test]
    fn same_name_suppressed() {
        let hints = hints_for(
            "void Foo(int nCount, string sName) {}",
            "#include \"lib\"\nvoid main() { int nCount = 5; string sName = \"x\"; Foo(nCount, sName); }",
        );
        // Both args match param names — all hints suppressed
        assert!(hints.is_empty());
    }

    #[test]
    fn case_insensitive_suppression() {
        let hints = hints_for(
            "void Foo(int nCount) {}",
            "#include \"lib\"\nvoid main() { int NCOUNT = 5; Foo(NCOUNT); }",
        );
        assert!(hints.is_empty());
    }

    #[test]
    fn nested_calls_get_hints() {
        let hints = hints_for(
            "int GetCount(object oPC) { return 0; }\nvoid SetValue(int nVal) {}",
            "#include \"lib\"\nvoid main() { SetValue(GetCount(OBJECT_SELF)); }",
        );
        assert_eq!(hints.len(), 2);
        assert_eq!(label_text(&hints[0]), "nVal:");
        assert_eq!(label_text(&hints[1]), "oPC:");
    }

    #[test]
    fn suppress_single_arg_setting() {
        let index = WorkspaceIndex::new(vec![], vec![], vec![]);
        let lib_uri = Url::parse("file:///lib.nss").unwrap();
        index.update_file(&lib_uri, "void Foo(int nCount) {}".to_string());

        let file_source = "#include \"lib\"\nvoid main() { Foo(42); }";
        let file_uri = Url::parse("file:///test.nss").unwrap();
        index.update_file(&file_uri, file_source.to_string());

        let parsed = nwscript_parser::parse(file_source);
        let line_index = LineIndex::new(file_source);

        // With suppression enabled
        let hints = inlay_hints(&parsed, &line_index, &index, &file_uri, true);
        assert!(hints.is_empty());

        // Without suppression
        let hints = inlay_hints(&parsed, &line_index, &index, &file_uri, false);
        assert_eq!(hints.len(), 1);
    }

    #[test]
    fn more_args_than_params_no_panic() {
        let hints = hints_for(
            "void Foo(int nA) {}",
            "#include \"lib\"\nvoid main() { Foo(1, 2, 3); }",
        );
        // Only the first arg gets a hint (params run out after that)
        assert_eq!(hints.len(), 1);
        assert_eq!(label_text(&hints[0]), "nA:");
    }

    #[test]
    fn hint_positions_are_correct() {
        let file_source = "#include \"lib\"\nvoid main() { Foo(42); }";
        let hints = hints_for("void Foo(int nX) {}", file_source);
        assert_eq!(hints.len(), 1);
        // "42" starts at column 18 on line 1 (0-indexed)
        assert_eq!(hints[0].position.line, 1);
        assert_eq!(hints[0].position.character, 18);
    }
}
