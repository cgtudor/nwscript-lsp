use crate::span::Span;
use crate::token::TokenKind;

/// A fully parsed NWScript file.
#[derive(Debug)]
pub struct ParsedFile {
    pub declarations: Vec<Declaration>,
    pub errors: Vec<ParseError>,
}

/// A parse error with location and message.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

// =============================================================================
// Top-level declarations
// =============================================================================

#[derive(Debug)]
pub enum Declaration {
    Include(IncludeDecl),
    Struct(StructDecl),
    Function(FunctionDecl),
    GlobalVar(VarDecl),
}

impl Declaration {
    pub fn span(&self) -> Span {
        match self {
            Declaration::Include(d) => d.span,
            Declaration::Struct(d) => d.span,
            Declaration::Function(d) => d.span,
            Declaration::GlobalVar(d) => d.span,
        }
    }
}

/// `#include "filename"`
#[derive(Debug)]
pub struct IncludeDecl {
    pub span: Span,
    pub path: Option<String>,
    pub path_span: Option<Span>,
}

/// `struct Name { type field; ... };`
#[derive(Debug)]
pub struct StructDecl {
    pub span: Span,
    pub name: Option<Ident>,
    pub fields: Vec<StructField>,
}

#[derive(Debug)]
pub struct StructField {
    pub span: Span,
    pub ty: TypeRef,
    pub name: Option<Ident>,
}

/// Function declaration (prototype) or definition (with body).
#[derive(Debug)]
pub struct FunctionDecl {
    pub span: Span,
    pub return_type: TypeRef,
    pub name: Option<Ident>,
    pub params: Vec<Param>,
    /// `None` for forward declarations (prototypes ending with `;`).
    pub body: Option<Block>,
}

impl FunctionDecl {
    pub fn is_prototype(&self) -> bool {
        self.body.is_none()
    }
}

/// A function parameter.
#[derive(Debug)]
pub struct Param {
    pub span: Span,
    pub ty: TypeRef,
    pub name: Option<Ident>,
    pub default_value: Option<Expr>,
}

/// Variable or constant declaration.
#[derive(Debug)]
pub struct VarDecl {
    pub span: Span,
    pub is_const: bool,
    pub ty: TypeRef,
    pub name: Option<Ident>,
    pub initializer: Option<Expr>,
}

// =============================================================================
// Types
// =============================================================================

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub span: Span,
    pub kind: TypeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Void,
    Int,
    Float,
    String,
    Object,
    Vector,
    Action,
    Effect,
    Event,
    ItemProperty,
    Location,
    Talent,
    Json,
    SqlQuery,
    Cassowary,
    Struct(String),
    /// Failed to parse type.
    Error,
}

impl TypeKind {
    pub fn from_token(kind: TokenKind) -> Option<Self> {
        match kind {
            TokenKind::KwVoid => Some(TypeKind::Void),
            TokenKind::KwInt => Some(TypeKind::Int),
            TokenKind::KwFloat => Some(TypeKind::Float),
            TokenKind::KwString => Some(TypeKind::String),
            TokenKind::KwObject => Some(TypeKind::Object),
            TokenKind::KwVector => Some(TypeKind::Vector),
            TokenKind::KwAction => Some(TypeKind::Action),
            TokenKind::KwEffect => Some(TypeKind::Effect),
            TokenKind::KwEvent => Some(TypeKind::Event),
            TokenKind::KwItemProperty => Some(TypeKind::ItemProperty),
            TokenKind::KwLocation => Some(TypeKind::Location),
            TokenKind::KwTalent => Some(TypeKind::Talent),
            TokenKind::KwJson => Some(TypeKind::Json),
            TokenKind::KwSqlQuery => Some(TypeKind::SqlQuery),
            TokenKind::KwCassowary => Some(TypeKind::Cassowary),
            _ => None,
        }
    }
}

// =============================================================================
// Identifiers
// =============================================================================

#[derive(Debug, Clone)]
pub struct Ident {
    pub span: Span,
    pub name: String,
}

// =============================================================================
// Statements
// =============================================================================

#[derive(Debug)]
pub struct Block {
    pub span: Span,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    VarDecl(VarDecl),
    Expr(ExprStmt),
    If(IfStmt),
    While(WhileStmt),
    DoWhile(DoWhileStmt),
    For(ForStmt),
    Switch(SwitchStmt),
    Return(ReturnStmt),
    Break(Span),
    Continue(Span),
    Block(Block),
    /// Lone semicolon.
    Empty(Span),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::VarDecl(s) => s.span,
            Stmt::Expr(s) => s.span,
            Stmt::If(s) => s.span,
            Stmt::While(s) => s.span,
            Stmt::DoWhile(s) => s.span,
            Stmt::For(s) => s.span,
            Stmt::Switch(s) => s.span,
            Stmt::Return(s) => s.span,
            Stmt::Break(s) | Stmt::Continue(s) | Stmt::Empty(s) => *s,
            Stmt::Block(s) => s.span,
        }
    }
}

#[derive(Debug)]
pub struct ExprStmt {
    pub span: Span,
    pub expr: Expr,
}

#[derive(Debug)]
pub struct IfStmt {
    pub span: Span,
    pub condition: Expr,
    pub then_branch: Box<Stmt>,
    pub else_branch: Option<Box<Stmt>>,
}

#[derive(Debug)]
pub struct WhileStmt {
    pub span: Span,
    pub condition: Expr,
    pub body: Box<Stmt>,
}

#[derive(Debug)]
pub struct DoWhileStmt {
    pub span: Span,
    pub body: Box<Stmt>,
    pub condition: Expr,
}

#[derive(Debug)]
pub struct ForStmt {
    pub span: Span,
    pub init: Option<Box<Stmt>>,
    pub condition: Option<Expr>,
    pub update: Option<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug)]
pub struct SwitchStmt {
    pub span: Span,
    pub expr: Expr,
    pub cases: Vec<SwitchCase>,
}

#[derive(Debug)]
pub struct SwitchCase {
    pub span: Span,
    pub label: CaseLabel,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub enum CaseLabel {
    Case(Expr),
    Default,
}

#[derive(Debug)]
pub struct ReturnStmt {
    pub span: Span,
    pub value: Option<Expr>,
}

// =============================================================================
// Expressions
// =============================================================================

#[derive(Debug)]
pub enum Expr {
    Literal(LiteralExpr),
    Ident(Ident),
    Binary(Box<BinaryExpr>),
    Unary(Box<UnaryExpr>),
    Postfix(Box<PostfixExpr>),
    Call(Box<CallExpr>),
    FieldAccess(Box<FieldAccessExpr>),
    Assignment(Box<AssignExpr>),
    Ternary(Box<TernaryExpr>),
    Paren(Box<Expr>),
    /// A vector literal: `[x, y, z]` or `Vector(x, y, z)`
    VectorLiteral(Box<VectorLiteralExpr>),
    /// Parse error placeholder.
    Error(Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(e) => e.span,
            Expr::Ident(e) => e.span,
            Expr::Binary(e) => e.span,
            Expr::Unary(e) => e.span,
            Expr::Postfix(e) => e.span,
            Expr::Call(e) => e.span,
            Expr::FieldAccess(e) => e.span,
            Expr::Assignment(e) => e.span,
            Expr::Ternary(e) => e.span,
            Expr::Paren(e) => e.span(),
            Expr::VectorLiteral(e) => e.span,
            Expr::Error(s) => *s,
        }
    }
}

#[derive(Debug)]
pub struct LiteralExpr {
    pub span: Span,
    pub kind: LiteralKind,
}

#[derive(Debug, Clone)]
pub enum LiteralKind {
    Int(i64),
    Float(f64),
    String(String),
    RawString(String),
    HashString(String),
}

#[derive(Debug)]
pub struct BinaryExpr {
    pub span: Span,
    pub left: Expr,
    pub op: BinaryOp,
    pub op_span: Span,
    pub right: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug)]
pub struct UnaryExpr {
    pub span: Span,
    pub op: UnaryOp,
    pub operand: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    PreInc,
    PreDec,
}

#[derive(Debug)]
pub struct PostfixExpr {
    pub span: Span,
    pub operand: Expr,
    pub op: PostfixOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOp {
    Inc,
    Dec,
}

#[derive(Debug)]
pub struct CallExpr {
    pub span: Span,
    pub callee: Expr,
    pub args: Vec<Expr>,
}

#[derive(Debug)]
pub struct FieldAccessExpr {
    pub span: Span,
    pub object: Expr,
    pub field: Ident,
}

#[derive(Debug)]
pub struct AssignExpr {
    pub span: Span,
    pub target: Expr,
    pub op: AssignOp,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
}

#[derive(Debug)]
pub struct TernaryExpr {
    pub span: Span,
    pub condition: Expr,
    pub then_expr: Expr,
    pub else_expr: Expr,
}

#[derive(Debug)]
pub struct VectorLiteralExpr {
    pub span: Span,
    pub x: Expr,
    pub y: Expr,
    pub z: Expr,
}
