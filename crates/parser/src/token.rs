use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn text<'s>(&self, source: &'s str) -> &'s str {
        self.span.text(source)
    }

    pub fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::LineComment | TokenKind::BlockComment
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // === Literals ===
    IntLiteral,
    HexLiteral,
    FloatLiteral,
    StringLiteral,
    RawStringLiteral,
    HashStringLiteral,

    // === Identifier ===
    Ident,

    // === Type keywords ===
    KwVoid,
    KwInt,
    KwFloat,
    KwString,
    KwObject,
    KwStruct,
    KwVector,
    KwAction,
    KwEffect,
    KwEvent,
    KwItemProperty,
    KwLocation,
    KwTalent,
    KwJson,
    KwSqlQuery,
    KwCassowary,

    // === Control flow keywords ===
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwDo,
    KwSwitch,
    KwCase,
    KwDefault,
    KwBreak,
    KwContinue,
    KwReturn,

    // === Other keywords ===
    KwConst,

    // === Preprocessor ===
    /// `#include`
    HashInclude,

    // === Operators ===
    Plus,        // +
    Minus,       // -
    Star,        // *
    Slash,       // /
    Percent,     // %
    Eq,          // =
    EqEq,        // ==
    BangEq,      // !=
    Lt,          // <
    Gt,          // >
    LtEq,        // <=
    GtEq,        // >=
    AmpAmp,      // &&
    PipePipe,    // ||
    Bang,        // !
    Amp,         // &
    Pipe,        // |
    Caret,       // ^
    Tilde,       // ~
    LtLt,        // <<
    GtGt,        // >>
    PlusEq,      // +=
    MinusEq,     // -=
    StarEq,      // *=
    SlashEq,     // /=
    PercentEq,   // %=
    PlusPlus,    // ++
    MinusMinus,  // --
    Question,    // ?
    Colon,       // :

    // === Delimiters ===
    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    LBracket, // [
    RBracket, // ]
    Semi,     // ;
    Comma,    // ,
    Dot,      // .

    // === Trivia ===
    Whitespace,
    Newline,
    LineComment,
    BlockComment,

    // === Special ===
    Error,
    Eof,
}

impl TokenKind {
    /// Look up a keyword from an identifier string.
    pub fn from_keyword(word: &str) -> Option<TokenKind> {
        match word {
            "void" => Some(TokenKind::KwVoid),
            "int" => Some(TokenKind::KwInt),
            "float" => Some(TokenKind::KwFloat),
            "string" => Some(TokenKind::KwString),
            "object" => Some(TokenKind::KwObject),
            "struct" => Some(TokenKind::KwStruct),
            "vector" => Some(TokenKind::KwVector),
            "action" => Some(TokenKind::KwAction),
            "effect" => Some(TokenKind::KwEffect),
            "event" => Some(TokenKind::KwEvent),
            "itemproperty" => Some(TokenKind::KwItemProperty),
            "location" => Some(TokenKind::KwLocation),
            "talent" => Some(TokenKind::KwTalent),
            "json" => Some(TokenKind::KwJson),
            "sqlquery" => Some(TokenKind::KwSqlQuery),
            "cassowary" => Some(TokenKind::KwCassowary),
            "if" => Some(TokenKind::KwIf),
            "else" => Some(TokenKind::KwElse),
            "while" => Some(TokenKind::KwWhile),
            "for" => Some(TokenKind::KwFor),
            "do" => Some(TokenKind::KwDo),
            "switch" => Some(TokenKind::KwSwitch),
            "case" => Some(TokenKind::KwCase),
            "default" => Some(TokenKind::KwDefault),
            "break" => Some(TokenKind::KwBreak),
            "continue" => Some(TokenKind::KwContinue),
            "return" => Some(TokenKind::KwReturn),
            "const" => Some(TokenKind::KwConst),
            _ => None,
        }
    }

    pub fn is_type_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::KwVoid
                | TokenKind::KwInt
                | TokenKind::KwFloat
                | TokenKind::KwString
                | TokenKind::KwObject
                | TokenKind::KwStruct
                | TokenKind::KwVector
                | TokenKind::KwAction
                | TokenKind::KwEffect
                | TokenKind::KwEvent
                | TokenKind::KwItemProperty
                | TokenKind::KwLocation
                | TokenKind::KwTalent
                | TokenKind::KwJson
                | TokenKind::KwSqlQuery
                | TokenKind::KwCassowary
        )
    }

    pub fn is_assignment_op(&self) -> bool {
        matches!(
            self,
            TokenKind::Eq
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
        )
    }
}
