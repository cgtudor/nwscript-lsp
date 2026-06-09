use nwscript_parser::ast::*;
use nwscript_parser::{LineIndex, ParsedFile};
use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenType, SemanticTokensLegend, SemanticTokensResult,
};

use crate::index::WorkspaceIndex;
use tower_lsp::lsp_types::Url;

/// Token types we emit. Order must match LEGEND_TYPE indices.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::FUNCTION,   // 0
    SemanticTokenType::VARIABLE,   // 1
    SemanticTokenType::PARAMETER,  // 2
    SemanticTokenType::TYPE,       // 3
    SemanticTokenType::STRUCT,     // 4
    SemanticTokenType::MACRO,      // 5 — used for constants
    SemanticTokenType::STRING,     // 6
    SemanticTokenType::NUMBER,     // 7
    SemanticTokenType::KEYWORD,    // 8
    SemanticTokenType::PROPERTY,   // 9 — used for struct fields
];

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: vec![],
    }
}

const TT_FUNCTION: u32 = 0;
const TT_VARIABLE: u32 = 1;
const TT_PARAMETER: u32 = 2;
const TT_TYPE: u32 = 3;
const TT_STRUCT: u32 = 4;
const TT_CONSTANT: u32 = 5;
const TT_PROPERTY: u32 = 9;

/// Compute semantic tokens for a file.
pub fn semantic_tokens(
    parsed: &ParsedFile,
    source: &str,
    line_index: &LineIndex,
    index: &WorkspaceIndex,
    uri: &Url,
) -> SemanticTokensResult {
    let mut collector = TokenCollector {
        source,
        line_index,
        tokens: Vec::new(),
        index,
        uri,
        local_params: Vec::new(),
        local_vars: Vec::new(),
    };

    for decl in &parsed.declarations {
        collector.visit_declaration(decl);
    }

    // Sort by position, then convert to deltas
    collector.tokens.sort_by_key(|t| (t.line, t.col));
    let delta_tokens = to_delta_tokens(&collector.tokens);

    SemanticTokensResult::Tokens(tower_lsp::lsp_types::SemanticTokens {
        result_id: None,
        data: delta_tokens,
    })
}

struct RawToken {
    line: u32,
    col: u32,
    length: u32,
    token_type: u32,
}

struct TokenCollector<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
    tokens: Vec<RawToken>,
    index: &'a WorkspaceIndex,
    uri: &'a Url,
    /// Parameters of the current function being visited.
    local_params: Vec<String>,
    /// Local variables of the current function being visited.
    local_vars: Vec<String>,
}

impl<'a> TokenCollector<'a> {
    fn push(&mut self, span: nwscript_parser::Span, token_type: u32) {
        if span.start >= span.end {
            return;
        }
        let (line, col) = self.line_index.line_col(span.start);
        let length = span.end - span.start;
        self.tokens.push(RawToken {
            line,
            col,
            length,
            token_type,
        });
    }

    fn visit_declaration(&mut self, decl: &Declaration) {
        match decl {
            Declaration::Include(_) => {}
            Declaration::Struct(s) => self.visit_struct(s),
            Declaration::Function(f) => self.visit_function(f),
            Declaration::GlobalVar(v) => self.visit_global_var(v),
        }
    }

    fn visit_type_ref(&mut self, ty: &TypeRef) {
        // Only emit semantic tokens for struct names in type positions.
        // Built-in types (int, float, object, etc.) are handled well by
        // the TextMate grammar as storage.type keywords.
        if let TypeKind::Struct(name) = &ty.kind {
            let text = &self.source[ty.span.start as usize..ty.span.end as usize];
            if let Some(offset) = text.find(name.as_str()) {
                let name_start = ty.span.start + offset as u32;
                let name_end = name_start + name.len() as u32;
                self.push(
                    nwscript_parser::Span {
                        start: name_start,
                        end: name_end,
                    },
                    TT_STRUCT,
                );
            }
        }
    }

    fn visit_struct(&mut self, s: &StructDecl) {
        if let Some(name) = &s.name {
            self.push(name.span, TT_STRUCT);
        }
        for field in &s.fields {
            self.visit_type_ref(&field.ty);
            if let Some(name) = &field.name {
                self.push(name.span, TT_PROPERTY);
            }
        }
    }

    fn visit_function(&mut self, f: &FunctionDecl) {
        self.visit_type_ref(&f.return_type);
        if let Some(name) = &f.name {
            self.push(name.span, TT_FUNCTION);
        }

        // Track parameters for the body
        self.local_params.clear();
        self.local_vars.clear();
        for param in &f.params {
            self.visit_type_ref(&param.ty);
            if let Some(name) = &param.name {
                self.push(name.span, TT_PARAMETER);
                self.local_params.push(name.name.clone());
            }
            if let Some(default) = &param.default_value {
                self.visit_expr(default);
            }
        }

        if let Some(body) = &f.body {
            self.visit_block(body);
        }

        self.local_params.clear();
        self.local_vars.clear();
    }

    fn visit_global_var(&mut self, v: &VarDecl) {
        // Don't emit tokens for the variable/constant name — the TextMate
        // grammar already handles ALL_CAPS constants well and theme colors
        // for MACRO/VARIABLE semantic tokens often clash.
        if let Some(init) = &v.initializer {
            self.visit_expr(init);
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
                // Track local vars for parameter highlighting but don't
                // emit tokens for the name — let TextMate handle it.
                if let Some(name) = &v.name {
                    if !v.is_const {
                        self.local_vars.push(name.name.clone());
                    }
                }
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
            Expr::Ident(ident) => {
                // Only classify identifiers we're certain about from local
                // context. Let the TextMate grammar handle everything else
                // (it does a better job for ALL_CAPS constants, etc.).
                let name = &ident.name;
                if self.local_params.iter().any(|p| p == name) {
                    self.push(ident.span, TT_PARAMETER);
                }
                // Don't emit for local vars or index lookups — TextMate
                // handles these well enough and the index lookup is expensive.
            }
            Expr::Call(c) => {
                // Highlight the callee as a function
                if let Expr::Ident(ident) = &c.callee {
                    self.push(ident.span, TT_FUNCTION);
                } else {
                    self.visit_expr(&c.callee);
                }
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
                self.push(f.field.span, TT_PROPERTY);
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
            Expr::Literal(_) | Expr::Error(_) => {}
        }
    }
}

/// Convert absolute tokens to LSP delta-encoded format.
fn to_delta_tokens(tokens: &[RawToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.col - prev_start
        } else {
            tok.col
        };

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = tok.line;
        prev_start = tok.col;
    }

    result
}
