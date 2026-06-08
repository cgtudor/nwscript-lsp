use crate::ast::*;
use crate::span::Span;
use crate::token::{Token, TokenKind};

/// Recursive descent parser for NWScript with Pratt expression parsing.
///
/// Produces partial ASTs even from broken code by recovering at synchronization
/// points (`;`, `}`, or declaration-starting keywords).
pub struct Parser<'src> {
    source: &'src str,
    tokens: Vec<Token>,
    /// Index into `tokens`, pointing at the next non-trivia token.
    pos: usize,
    errors: Vec<ParseError>,
    /// Extra statements produced by comma-separated var declarations.
    pending_stmts: Vec<Stmt>,
}

impl<'src> Parser<'src> {
    pub fn parse(source: &'src str, tokens: Vec<Token>) -> ParsedFile {
        let mut parser = Self {
            source,
            tokens,
            pos: 0,
            errors: Vec::new(),
            pending_stmts: Vec::new(),
        };

        // Advance past any leading trivia.
        parser.skip_trivia();

        let mut declarations = Vec::new();
        while !parser.at_end() {
            if let Some(decl) = parser.parse_declaration() {
                declarations.push(decl);
            }
        }

        ParsedFile {
            declarations,
            errors: parser.errors,
        }
    }

    // =========================================================================
    // Top-level declarations
    // =========================================================================

    fn parse_declaration(&mut self) -> Option<Declaration> {
        // #include
        if self.at(TokenKind::HashInclude) {
            return Some(Declaration::Include(self.parse_include()));
        }

        // const ...
        if self.at(TokenKind::KwConst) {
            return Some(Declaration::GlobalVar(self.parse_const_var()));
        }

        // struct definition: `struct Name { ... };`
        if self.at(TokenKind::KwStruct) && self.peek_kind(1) == Some(TokenKind::Ident) {
            // Look ahead: is this `struct Name {` (definition) or `struct Name func(` (return type)?
            if self.peek_kind(2) == Some(TokenKind::LBrace) {
                return Some(Declaration::Struct(self.parse_struct()));
            }
        }

        // Function or global variable: starts with a type.
        if self.at_type_start() {
            return self.parse_function_or_var();
        }

        // Error recovery: skip unknown token.
        let tok = self.current();
        self.error_at(tok.span, format!("unexpected token `{}`", tok.text(self.source)));
        self.advance();
        None
    }

    fn parse_include(&mut self) -> IncludeDecl {
        let start = self.current().span;
        self.expect(TokenKind::HashInclude);

        let (path, path_span) = if self.at(TokenKind::StringLiteral) {
            let tok = self.current();
            let raw = tok.text(self.source);
            // Strip quotes
            let path = raw[1..raw.len() - 1].to_string();
            let sp = tok.span;
            self.advance();
            (Some(path), Some(sp))
        } else {
            self.error("expected string literal after #include");
            (None, None)
        };

        let span = match path_span {
            Some(ps) => start.merge(ps),
            None => start,
        };

        IncludeDecl {
            span,
            path,
            path_span,
        }
    }

    fn parse_struct(&mut self) -> StructDecl {
        let start = self.current().span;
        self.expect(TokenKind::KwStruct);

        let name = self.parse_ident();

        let mut fields = Vec::new();
        if self.eat(TokenKind::LBrace) {
            while !self.at(TokenKind::RBrace) && !self.at_end() {
                if let Some(field) = self.parse_struct_field() {
                    fields.push(field);
                } else {
                    // Recovery: skip to next ; or }
                    self.recover_to(&[TokenKind::Semi, TokenKind::RBrace]);
                    self.eat(TokenKind::Semi);
                }
            }
            self.expect(TokenKind::RBrace);
        }
        self.eat(TokenKind::Semi);

        let end = self.prev_span();
        StructDecl {
            span: start.merge(end),
            name,
            fields,
        }
    }

    fn parse_struct_field(&mut self) -> Option<StructField> {
        let start = self.current().span;
        let ty = self.parse_type()?;
        let name = self.parse_ident();
        self.expect(TokenKind::Semi);
        let end = self.prev_span();
        Some(StructField {
            span: start.merge(end),
            ty,
            name,
        })
    }

    fn parse_const_var(&mut self) -> VarDecl {
        let start = self.current().span;
        self.expect(TokenKind::KwConst);

        let ty = self.parse_type().unwrap_or(TypeRef {
            span: self.current().span,
            kind: TypeKind::Error,
        });
        let name = self.parse_ident();

        let initializer = if self.eat(TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };

        self.expect(TokenKind::Semi);
        let end = self.prev_span();

        VarDecl {
            span: start.merge(end),
            is_const: true,
            ty,
            name,
            initializer,
        }
    }

    /// Parse either a function declaration/definition or a global variable declaration.
    /// Both start with `type name`.
    fn parse_function_or_var(&mut self) -> Option<Declaration> {
        let start = self.current().span;
        let ty = self.parse_type()?;
        let name = self.parse_ident();

        // Function: name followed by `(`
        if self.at(TokenKind::LParen) {
            return Some(Declaration::Function(self.parse_function_rest(start, ty, name)));
        }

        // Global variable
        let initializer = if self.eat(TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenKind::Semi);
        let end = self.prev_span();

        Some(Declaration::GlobalVar(VarDecl {
            span: start.merge(end),
            is_const: false,
            ty,
            name,
            initializer,
        }))
    }

    fn parse_function_rest(
        &mut self,
        start: Span,
        return_type: TypeRef,
        name: Option<Ident>,
    ) -> FunctionDecl {
        // Parse parameters
        self.expect(TokenKind::LParen);
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen);

        // Body or semicolon
        let body = if self.at(TokenKind::LBrace) {
            Some(self.parse_block())
        } else {
            self.expect(TokenKind::Semi);
            None
        };

        let end = self.prev_span();
        FunctionDecl {
            span: start.merge(end),
            return_type,
            name,
            params,
            body,
        }
    }

    fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if self.at(TokenKind::RParen) {
            return params;
        }

        loop {
            if let Some(param) = self.parse_param() {
                params.push(param);
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        params
    }

    fn parse_param(&mut self) -> Option<Param> {
        let start = self.current().span;
        let ty = self.parse_type()?;
        let name = self.parse_ident();

        let default_value = if self.eat(TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };

        let end = self.prev_span();
        Some(Param {
            span: start.merge(end),
            ty,
            name,
            default_value,
        })
    }

    // =========================================================================
    // Types
    // =========================================================================

    fn parse_type(&mut self) -> Option<TypeRef> {
        let tok = self.current();

        // `struct StructName`
        if tok.kind == TokenKind::KwStruct {
            let start = tok.span;
            self.advance();
            if self.at(TokenKind::Ident) {
                let name_tok = self.current();
                let name = name_tok.text(self.source).to_string();
                let end = name_tok.span;
                self.advance();
                return Some(TypeRef {
                    span: start.merge(end),
                    kind: TypeKind::Struct(name),
                });
            } else {
                self.error("expected struct name");
                return Some(TypeRef {
                    span: start,
                    kind: TypeKind::Error,
                });
            }
        }

        // Built-in type keywords
        if let Some(type_kind) = TypeKind::from_token(tok.kind) {
            let span = tok.span;
            self.advance();
            return Some(TypeRef {
                span,
                kind: type_kind,
            });
        }

        None
    }

    fn at_type_start(&self) -> bool {
        let kind = self.current().kind;
        kind.is_type_keyword() || kind == TokenKind::KwStruct
    }

    // =========================================================================
    // Statements
    // =========================================================================

    fn parse_block(&mut self) -> Block {
        let start = self.current().span;
        self.expect(TokenKind::LBrace);

        let mut stmts = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at_end() {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
                // Drain any extra stmts from comma-separated var decls
                stmts.append(&mut self.pending_stmts);
            }
        }

        self.expect(TokenKind::RBrace);
        let end = self.prev_span();

        Block {
            span: start.merge(end),
            stmts,
        }
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match self.current().kind {
            TokenKind::Semi => {
                let span = self.current().span;
                self.advance();
                Some(Stmt::Empty(span))
            }
            TokenKind::LBrace => Some(Stmt::Block(self.parse_block())),
            TokenKind::KwIf => Some(Stmt::If(self.parse_if())),
            TokenKind::KwWhile => Some(Stmt::While(self.parse_while())),
            TokenKind::KwDo => Some(Stmt::DoWhile(self.parse_do_while())),
            TokenKind::KwFor => Some(Stmt::For(self.parse_for())),
            TokenKind::KwSwitch => Some(Stmt::Switch(self.parse_switch())),
            TokenKind::KwReturn => Some(Stmt::Return(self.parse_return())),
            TokenKind::KwBreak => {
                let span = self.current().span;
                self.advance();
                let end = self.current().span;
                self.expect(TokenKind::Semi);
                Some(Stmt::Break(span.merge(end)))
            }
            TokenKind::KwContinue => {
                let span = self.current().span;
                self.advance();
                let end = self.current().span;
                self.expect(TokenKind::Semi);
                Some(Stmt::Continue(span.merge(end)))
            }
            TokenKind::KwConst => Some(Stmt::VarDecl(self.parse_const_var())),
            _ => {
                // Variable declaration or expression statement.
                // Variable decl starts with type keyword or `struct`.
                if self.at_type_start() && !self.at(TokenKind::KwVoid) {
                    // Could be var decl or expression (e.g., `object` could be a type).
                    // Look ahead: type followed by ident is a var decl.
                    if self.is_var_decl_lookahead() {
                        // Comma-separated: `int a, b = 5;` → multiple VarDecl stmts
                        let decls = self.parse_local_var_list();
                        // Return first; push rest into pending_stmts
                        let mut iter = decls.into_iter();
                        let first = iter.next().unwrap();
                        for extra in iter {
                            self.pending_stmts.push(Stmt::VarDecl(extra));
                        }
                        return Some(Stmt::VarDecl(first));
                    }
                }

                // Expression statement
                let start = self.current().span;
                let expr = self.parse_expr();
                self.expect(TokenKind::Semi);
                let end = self.prev_span();
                Some(Stmt::Expr(ExprStmt {
                    span: start.merge(end),
                    expr,
                }))
            }
        }
    }

    /// Disambiguate `type name ...` (var decl) from expression.
    fn is_var_decl_lookahead(&self) -> bool {
        // `struct Name name` -> var decl
        if self.current().kind == TokenKind::KwStruct {
            return self.peek_kind(1) == Some(TokenKind::Ident)
                && self.peek_kind(2) == Some(TokenKind::Ident);
        }
        // `int x`, `string s`, etc.
        self.current().kind.is_type_keyword() && self.peek_kind(1) == Some(TokenKind::Ident)
            && self.peek_kind(2) != Some(TokenKind::LParen)
    }

    /// Parse a local variable declaration, possibly comma-separated.
    /// `int a, b = 5, c;` produces multiple VarDecl nodes.
    fn parse_local_var_list(&mut self) -> Vec<VarDecl> {
        let start = self.current().span;
        let ty = self.parse_type().unwrap_or(TypeRef {
            span: self.current().span,
            kind: TypeKind::Error,
        });

        let mut decls = Vec::new();

        // First variable
        let name = self.parse_ident();
        let initializer = if self.eat(TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };
        let end = self.prev_span();
        decls.push(VarDecl {
            span: start.merge(end),
            is_const: false,
            ty: ty.clone(),
            name,
            initializer,
        });

        // Additional comma-separated variables
        while self.eat(TokenKind::Comma) {
            let var_start = self.current().span;
            let name = self.parse_ident();
            let initializer = if self.eat(TokenKind::Eq) {
                Some(self.parse_expr())
            } else {
                None
            };
            let var_end = self.prev_span();
            decls.push(VarDecl {
                span: var_start.merge(var_end),
                is_const: false,
                ty: ty.clone(),
                name,
                initializer,
            });
        }

        self.expect(TokenKind::Semi);
        decls
    }

    fn parse_if(&mut self) -> IfStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwIf);
        self.expect(TokenKind::LParen);
        let condition = self.parse_expr();
        self.expect(TokenKind::RParen);
        let then_branch = self
            .parse_stmt()
            .unwrap_or(Stmt::Empty(self.current().span));
        let else_branch = if self.eat(TokenKind::KwElse) {
            Some(Box::new(
                self.parse_stmt()
                    .unwrap_or(Stmt::Empty(self.current().span)),
            ))
        } else {
            None
        };
        let end = self.prev_span();
        IfStmt {
            span: start.merge(end),
            condition,
            then_branch: Box::new(then_branch),
            else_branch,
        }
    }

    fn parse_while(&mut self) -> WhileStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwWhile);
        self.expect(TokenKind::LParen);
        let condition = self.parse_expr();
        self.expect(TokenKind::RParen);
        let body = self
            .parse_stmt()
            .unwrap_or(Stmt::Empty(self.current().span));
        let end = self.prev_span();
        WhileStmt {
            span: start.merge(end),
            condition,
            body: Box::new(body),
        }
    }

    fn parse_do_while(&mut self) -> DoWhileStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwDo);
        let body = self
            .parse_stmt()
            .unwrap_or(Stmt::Empty(self.current().span));
        self.expect(TokenKind::KwWhile);
        self.expect(TokenKind::LParen);
        let condition = self.parse_expr();
        self.expect(TokenKind::RParen);
        self.expect(TokenKind::Semi);
        let end = self.prev_span();
        DoWhileStmt {
            span: start.merge(end),
            body: Box::new(body),
            condition,
        }
    }

    fn parse_for(&mut self) -> ForStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwFor);
        self.expect(TokenKind::LParen);

        // Init
        let init = if self.at(TokenKind::Semi) {
            self.advance();
            None
        } else if self.at_type_start() && self.is_var_decl_lookahead() {
            let decls = self.parse_local_var_list();
            let first = decls.into_iter().next().unwrap();
            Some(Box::new(Stmt::VarDecl(first)))
        } else {
            let expr = self.parse_expr();
            let sp = expr.span();
            self.expect(TokenKind::Semi);
            Some(Box::new(Stmt::Expr(ExprStmt {
                span: sp,
                expr,
            })))
        };

        // Condition
        let condition = if self.at(TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(TokenKind::Semi);

        // Update
        let update = if self.at(TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(TokenKind::RParen);

        let body = self
            .parse_stmt()
            .unwrap_or(Stmt::Empty(self.current().span));
        let end = self.prev_span();

        ForStmt {
            span: start.merge(end),
            init,
            condition,
            update,
            body: Box::new(body),
        }
    }

    fn parse_switch(&mut self) -> SwitchStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwSwitch);
        self.expect(TokenKind::LParen);
        let expr = self.parse_expr();
        self.expect(TokenKind::RParen);
        self.expect(TokenKind::LBrace);

        let mut cases = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at_end() {
            if self.at(TokenKind::KwCase) || self.at(TokenKind::KwDefault) {
                cases.push(self.parse_switch_case());
            } else {
                // Recovery
                self.error("expected `case` or `default`");
                self.advance();
            }
        }
        self.expect(TokenKind::RBrace);
        let end = self.prev_span();

        SwitchStmt {
            span: start.merge(end),
            expr,
            cases,
        }
    }

    fn parse_switch_case(&mut self) -> SwitchCase {
        let start = self.current().span;
        let label = if self.eat(TokenKind::KwDefault) {
            CaseLabel::Default
        } else {
            self.expect(TokenKind::KwCase);
            let expr = self.parse_expr();
            CaseLabel::Case(expr)
        };
        self.expect(TokenKind::Colon);

        let mut stmts = Vec::new();
        while !self.at(TokenKind::KwCase)
            && !self.at(TokenKind::KwDefault)
            && !self.at(TokenKind::RBrace)
            && !self.at_end()
        {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
        }

        let end = self.prev_span();
        SwitchCase {
            span: start.merge(end),
            label,
            stmts,
        }
    }

    fn parse_return(&mut self) -> ReturnStmt {
        let start = self.current().span;
        self.expect(TokenKind::KwReturn);

        let value = if self.at(TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(TokenKind::Semi);
        let end = self.prev_span();

        ReturnStmt {
            span: start.merge(end),
            value,
        }
    }

    // =========================================================================
    // Expressions — Pratt parser
    // =========================================================================

    fn parse_expr(&mut self) -> Expr {
        self.parse_assignment_expr()
    }

    fn parse_assignment_expr(&mut self) -> Expr {
        let expr = self.parse_ternary_expr();

        if self.current().kind.is_assignment_op() {
            let op_tok = self.current();
            let op = match op_tok.kind {
                TokenKind::Eq => AssignOp::Assign,
                TokenKind::PlusEq => AssignOp::AddAssign,
                TokenKind::MinusEq => AssignOp::SubAssign,
                TokenKind::StarEq => AssignOp::MulAssign,
                TokenKind::SlashEq => AssignOp::DivAssign,
                TokenKind::PercentEq => AssignOp::ModAssign,
                _ => unreachable!(),
            };
            self.advance();
            let value = self.parse_assignment_expr();
            let span = expr.span().merge(value.span());
            return Expr::Assignment(Box::new(AssignExpr {
                span,
                target: expr,
                op,
                value,
            }));
        }

        expr
    }

    fn parse_ternary_expr(&mut self) -> Expr {
        let expr = self.parse_binary_expr(0);

        if self.eat(TokenKind::Question) {
            let then_expr = self.parse_expr();
            self.expect(TokenKind::Colon);
            let else_expr = self.parse_ternary_expr();
            let span = expr.span().merge(else_expr.span());
            return Expr::Ternary(Box::new(TernaryExpr {
                span,
                condition: expr,
                then_expr,
                else_expr,
            }));
        }

        expr
    }

    /// Pratt parser for binary expressions.
    fn parse_binary_expr(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary_expr();

        loop {
            let Some((op, prec)) = self.binary_op_prec() else {
                break;
            };
            if prec < min_prec {
                break;
            }

            let op_span = self.current().span;
            self.advance();
            let right = self.parse_binary_expr(prec + 1);
            let span = left.span().merge(right.span());
            left = Expr::Binary(Box::new(BinaryExpr {
                span,
                left,
                op,
                op_span,
                right,
            }));
        }

        left
    }

    fn binary_op_prec(&self) -> Option<(BinaryOp, u8)> {
        match self.current().kind {
            TokenKind::PipePipe => Some((BinaryOp::Or, 1)),
            TokenKind::AmpAmp => Some((BinaryOp::And, 2)),
            TokenKind::Pipe => Some((BinaryOp::BitOr, 3)),
            TokenKind::Caret => Some((BinaryOp::BitXor, 4)),
            TokenKind::Amp => Some((BinaryOp::BitAnd, 5)),
            TokenKind::EqEq => Some((BinaryOp::Eq, 6)),
            TokenKind::BangEq => Some((BinaryOp::Neq, 6)),
            TokenKind::Lt => Some((BinaryOp::Lt, 7)),
            TokenKind::Gt => Some((BinaryOp::Gt, 7)),
            TokenKind::LtEq => Some((BinaryOp::LtEq, 7)),
            TokenKind::GtEq => Some((BinaryOp::GtEq, 7)),
            TokenKind::LtLt => Some((BinaryOp::Shl, 8)),
            TokenKind::GtGt => Some((BinaryOp::Shr, 8)),
            TokenKind::Plus => Some((BinaryOp::Add, 9)),
            TokenKind::Minus => Some((BinaryOp::Sub, 9)),
            TokenKind::Star => Some((BinaryOp::Mul, 10)),
            TokenKind::Slash => Some((BinaryOp::Div, 10)),
            TokenKind::Percent => Some((BinaryOp::Mod, 10)),
            _ => None,
        }
    }

    fn parse_unary_expr(&mut self) -> Expr {
        let tok = self.current();
        match tok.kind {
            TokenKind::Minus => {
                let start = tok.span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start.merge(operand.span());
                Expr::Unary(Box::new(UnaryExpr {
                    span,
                    op: UnaryOp::Neg,
                    operand,
                }))
            }
            TokenKind::Bang => {
                let start = tok.span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start.merge(operand.span());
                Expr::Unary(Box::new(UnaryExpr {
                    span,
                    op: UnaryOp::Not,
                    operand,
                }))
            }
            TokenKind::Tilde => {
                let start = tok.span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start.merge(operand.span());
                Expr::Unary(Box::new(UnaryExpr {
                    span,
                    op: UnaryOp::BitNot,
                    operand,
                }))
            }
            TokenKind::PlusPlus => {
                let start = tok.span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start.merge(operand.span());
                Expr::Unary(Box::new(UnaryExpr {
                    span,
                    op: UnaryOp::PreInc,
                    operand,
                }))
            }
            TokenKind::MinusMinus => {
                let start = tok.span;
                self.advance();
                let operand = self.parse_unary_expr();
                let span = start.merge(operand.span());
                Expr::Unary(Box::new(UnaryExpr {
                    span,
                    op: UnaryOp::PreDec,
                    operand,
                }))
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> Expr {
        let mut expr = self.parse_primary_expr();

        loop {
            match self.current().kind {
                TokenKind::PlusPlus => {
                    let end = self.current().span;
                    self.advance();
                    let span = expr.span().merge(end);
                    expr = Expr::Postfix(Box::new(PostfixExpr {
                        span,
                        operand: expr,
                        op: PostfixOp::Inc,
                    }));
                }
                TokenKind::MinusMinus => {
                    let end = self.current().span;
                    self.advance();
                    let span = expr.span().merge(end);
                    expr = Expr::Postfix(Box::new(PostfixExpr {
                        span,
                        operand: expr,
                        op: PostfixOp::Dec,
                    }));
                }
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_arg_list();
                    let end_span = self.current().span;
                    self.expect(TokenKind::RParen);
                    let span = expr.span().merge(end_span);
                    expr = Expr::Call(Box::new(CallExpr {
                        span,
                        callee: expr,
                        args,
                    }));
                }
                TokenKind::Dot => {
                    self.advance();
                    let field = self.parse_ident().unwrap_or(Ident {
                        span: self.current().span,
                        name: String::new(),
                    });
                    let span = expr.span().merge(field.span);
                    expr = Expr::FieldAccess(Box::new(FieldAccessExpr {
                        span,
                        object: expr,
                        field,
                    }));
                }
                _ => break,
            }
        }

        expr
    }

    fn parse_primary_expr(&mut self) -> Expr {
        let tok = self.current();

        match tok.kind {
            TokenKind::IntLiteral | TokenKind::HexLiteral => {
                let span = tok.span;
                let text = tok.text(self.source);
                let value = if tok.kind == TokenKind::HexLiteral {
                    i64::from_str_radix(text.trim_start_matches("0x").trim_start_matches("0X"), 16)
                        .unwrap_or(0)
                } else {
                    text.parse::<i64>().unwrap_or(0)
                };
                self.advance();
                Expr::Literal(LiteralExpr {
                    span,
                    kind: LiteralKind::Int(value),
                })
            }
            TokenKind::FloatLiteral => {
                let span = tok.span;
                let text = tok.text(self.source).trim_end_matches('f').trim_end_matches('F');
                let value = text.parse::<f64>().unwrap_or(0.0);
                self.advance();
                Expr::Literal(LiteralExpr {
                    span,
                    kind: LiteralKind::Float(value),
                })
            }
            TokenKind::StringLiteral => {
                let span = tok.span;
                let raw = tok.text(self.source);
                let value = raw[1..raw.len().saturating_sub(1)].to_string();
                self.advance();
                Expr::Literal(LiteralExpr {
                    span,
                    kind: LiteralKind::String(value),
                })
            }
            TokenKind::RawStringLiteral => {
                let span = tok.span;
                let raw = tok.text(self.source);
                // r"..." -> strip r" and "
                let value = raw[2..raw.len().saturating_sub(1)].to_string();
                self.advance();
                Expr::Literal(LiteralExpr {
                    span,
                    kind: LiteralKind::RawString(value),
                })
            }
            TokenKind::HashStringLiteral => {
                let span = tok.span;
                let raw = tok.text(self.source);
                let value = raw[2..raw.len().saturating_sub(1)].to_string();
                self.advance();
                Expr::Literal(LiteralExpr {
                    span,
                    kind: LiteralKind::HashString(value),
                })
            }
            TokenKind::Ident => {
                let ident = self.parse_ident().unwrap();
                Expr::Ident(ident)
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr();
                self.expect(TokenKind::RParen);
                Expr::Paren(Box::new(inner))
            }
            TokenKind::LBracket => {
                // Vector literal: [x, y, z]
                let start = tok.span;
                self.advance();
                let x = self.parse_expr();
                self.expect(TokenKind::Comma);
                let y = self.parse_expr();
                self.expect(TokenKind::Comma);
                let z = self.parse_expr();
                let end = self.current().span;
                self.expect(TokenKind::RBracket);
                Expr::VectorLiteral(Box::new(VectorLiteralExpr {
                    span: start.merge(end),
                    x,
                    y,
                    z,
                }))
            }
            _ => {
                let span = tok.span;
                self.error_at(span, format!("expected expression, found `{}`", tok.text(self.source).chars().take(20).collect::<String>()));
                self.advance();
                Expr::Error(span)
            }
        }
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        if self.at(TokenKind::RParen) {
            return args;
        }

        loop {
            args.push(self.parse_expr());
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        args
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn parse_ident(&mut self) -> Option<Ident> {
        if self.at(TokenKind::Ident) {
            let tok = self.current();
            let ident = Ident {
                span: tok.span,
                name: tok.text(self.source).to_string(),
            };
            self.advance();
            Some(ident)
        } else {
            self.error("expected identifier");
            None
        }
    }

    // =========================================================================
    // Token navigation (trivia-skipping)
    // =========================================================================

    fn current(&self) -> Token {
        self.tokens
            .get(self.pos)
            .copied()
            .unwrap_or(Token::new(TokenKind::Eof, Span::empty(self.source.len())))
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn at_end(&self) -> bool {
        self.current().kind == TokenKind::Eof
    }

    /// Peek at the Nth non-trivia token ahead (0 = current).
    fn peek_kind(&self, n: usize) -> Option<TokenKind> {
        let mut count = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            if !self.tokens[i].is_trivia() {
                if count == n {
                    return Some(self.tokens[i].kind);
                }
                count += 1;
            }
            i += 1;
        }
        None
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        self.skip_trivia();
    }

    fn skip_trivia(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].is_trivia() {
            self.pos += 1;
        }
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) {
        if !self.eat(kind) {
            let expected = token_display(kind);
            let found = self.current();
            let found_text = if found.kind == TokenKind::Eof {
                "end of file".to_string()
            } else {
                format!("`{}`", found.text(self.source))
            };
            self.error(&format!("expected {expected}, found {found_text}"));
        }
    }

    fn prev_span(&self) -> Span {
        // Walk backwards from current pos to find last non-trivia token.
        let mut i = self.pos.saturating_sub(1);
        while i > 0 && self.tokens[i].is_trivia() {
            i -= 1;
        }
        self.tokens
            .get(i)
            .map(|t| t.span)
            .unwrap_or(Span::empty(0))
    }

    // =========================================================================
    // Error handling and recovery
    // =========================================================================

    fn error(&mut self, msg: &str) {
        let span = self.current().span;
        self.error_at(span, msg.to_string());
    }

    fn error_at(&mut self, span: Span, msg: String) {
        self.errors.push(ParseError { span, message: msg });
    }

    /// Skip tokens until we reach one of the given synchronization tokens.
    fn recover_to(&mut self, sync: &[TokenKind]) {
        while !self.at_end() {
            if sync.contains(&self.current().kind) {
                return;
            }
            self.advance();
        }
    }
}

/// Human-readable display for expected tokens in error messages.
fn token_display(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Semi => "`;`",
        TokenKind::Comma => "`,`",
        TokenKind::LParen => "`(`",
        TokenKind::RParen => "`)`",
        TokenKind::LBrace => "`{`",
        TokenKind::RBrace => "`}`",
        TokenKind::LBracket => "`[`",
        TokenKind::RBracket => "`]`",
        TokenKind::Colon => "`:`",
        TokenKind::Dot => "`.`",
        TokenKind::Eq => "`=`",
        TokenKind::HashInclude => "`#include`",
        TokenKind::KwIf => "`if`",
        TokenKind::KwElse => "`else`",
        TokenKind::KwWhile => "`while`",
        TokenKind::KwFor => "`for`",
        TokenKind::KwReturn => "`return`",
        TokenKind::KwCase => "`case`",
        TokenKind::KwDefault => "`default`",
        TokenKind::KwStruct => "`struct`",
        TokenKind::Ident => "identifier",
        _ => "token",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> ParsedFile {
        let tokens = Lexer::tokenize(src);
        Parser::parse(src, tokens)
    }

    #[test]
    fn parse_include() {
        let file = parse(r#"#include "nwnx_player""#);
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        assert_eq!(file.declarations.len(), 1);
        match &file.declarations[0] {
            Declaration::Include(inc) => {
                assert_eq!(inc.path.as_deref(), Some("nwnx_player"));
            }
            other => panic!("expected Include, got {other:?}"),
        }
    }

    #[test]
    fn parse_struct() {
        let file = parse("struct Foo { int x; string y; };");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        assert_eq!(file.declarations.len(), 1);
        match &file.declarations[0] {
            Declaration::Struct(s) => {
                assert_eq!(s.name.as_ref().unwrap().name, "Foo");
                assert_eq!(s.fields.len(), 2);
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    #[test]
    fn parse_function_prototype() {
        let file = parse("void DoStuff(int nArg, string sName = \"test\");");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        assert_eq!(file.declarations.len(), 1);
        match &file.declarations[0] {
            Declaration::Function(f) => {
                assert!(f.is_prototype());
                assert_eq!(f.name.as_ref().unwrap().name, "DoStuff");
                assert_eq!(f.params.len(), 2);
                assert!(f.params[1].default_value.is_some());
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn parse_function_definition() {
        let file = parse("void main() { int x = 42; return; }");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        match &file.declarations[0] {
            Declaration::Function(f) => {
                assert!(!f.is_prototype());
                assert_eq!(f.name.as_ref().unwrap().name, "main");
                let body = f.body.as_ref().unwrap();
                assert_eq!(body.stmts.len(), 2);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn parse_global_const() {
        let file = parse("const int FOO = 42;");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        match &file.declarations[0] {
            Declaration::GlobalVar(v) => {
                assert!(v.is_const);
                assert_eq!(v.name.as_ref().unwrap().name, "FOO");
            }
            other => panic!("expected GlobalVar, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_expression() {
        let file = parse("void main() { int x = a + b * c; }");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
    }

    #[test]
    fn error_recovery() {
        // Missing semicolon — parser should recover
        let file = parse("void main() { int x = 42 return; }");
        assert!(!file.errors.is_empty());
        // Should still produce some declarations
        assert_eq!(file.declarations.len(), 1);
    }

    #[test]
    fn parse_comma_separated_vars() {
        let file = parse("void main() { string sA, sB = \"x\", sC; }");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
        match &file.declarations[0] {
            Declaration::Function(f) => {
                let body = f.body.as_ref().unwrap();
                // Should produce 3 separate VarDecl statements
                assert_eq!(body.stmts.len(), 3, "stmts: {:#?}", body.stmts);
                assert!(matches!(&body.stmts[0], Stmt::VarDecl(v) if v.name.as_ref().unwrap().name == "sA"));
                assert!(matches!(&body.stmts[1], Stmt::VarDecl(v) if v.name.as_ref().unwrap().name == "sB"));
                assert!(matches!(&body.stmts[2], Stmt::VarDecl(v) if v.name.as_ref().unwrap().name == "sC"));
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn parse_for_with_bare_init() {
        // for (n; n < 10; n++) — bare ident as init
        let file = parse("void main() { int n; for (n; n < 10; n++) { } }");
        assert!(file.errors.is_empty(), "errors: {:?}", file.errors);
    }
}
