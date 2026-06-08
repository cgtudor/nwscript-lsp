use crate::span::Span;
use crate::token::{Token, TokenKind};

/// Hand-written lexer for NWScript.
///
/// Produces all tokens including trivia (whitespace, comments) so that
/// the parser can skip them while preserving comment info for hover docs.
pub struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
}

impl<'src> Lexer<'src> {
    pub fn tokenize(source: &'src str) -> Vec<Token> {
        let mut lexer = Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        };

        let mut tokens = Vec::with_capacity(source.len() / 4);
        loop {
            let token = lexer.next_token();
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn next_token(&mut self) -> Token {
        if self.pos >= self.bytes.len() {
            return Token::new(TokenKind::Eof, Span::empty(self.pos));
        }

        let start = self.pos;
        let b = self.bytes[self.pos];

        match b {
            // Whitespace (not newlines)
            b' ' | b'\t' | b'\r' => self.scan_whitespace(start),

            // Newlines
            b'\n' => {
                self.pos += 1;
                Token::new(TokenKind::Newline, Span::new(start, self.pos))
            }

            // Slash: comment or division
            b'/' => {
                if self.peek() == Some(b'/') {
                    self.scan_line_comment(start)
                } else if self.peek() == Some(b'*') {
                    self.scan_block_comment(start)
                } else if self.peek() == Some(b'=') {
                    self.pos += 2;
                    Token::new(TokenKind::SlashEq, Span::new(start, self.pos))
                } else {
                    self.pos += 1;
                    Token::new(TokenKind::Slash, Span::new(start, self.pos))
                }
            }

            // Hash: preprocessor directive
            b'#' => self.scan_preprocessor(start),

            // String literals
            b'"' => self.scan_string(start),

            // Numbers
            b'0' if self.peek() == Some(b'x') || self.peek() == Some(b'X') => {
                self.scan_hex_number(start)
            }
            b'0'..=b'9' => self.scan_number(start),

            // Dot: could be float (.5) or member access
            b'.' if self.peek().is_some_and(|b| b.is_ascii_digit()) => self.scan_number(start),
            b'.' => {
                self.pos += 1;
                Token::new(TokenKind::Dot, Span::new(start, self.pos))
            }

            // Identifiers and keywords (including r"..." and h"..." prefixed strings)
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                // Check for r"..." raw strings and h"..." hash strings
                if b == b'r' && self.peek() == Some(b'"') {
                    self.pos += 1; // skip 'r'
                    let mut tok = self.scan_string(start);
                    tok.kind = TokenKind::RawStringLiteral;
                    tok.span = Span::new(start, tok.span.end as usize);
                    tok
                } else if b == b'h' && self.peek() == Some(b'"') {
                    self.pos += 1; // skip 'h'
                    let mut tok = self.scan_string(start);
                    tok.kind = TokenKind::HashStringLiteral;
                    tok.span = Span::new(start, tok.span.end as usize);
                    tok
                } else {
                    self.scan_identifier(start)
                }
            }

            // Operators and punctuation
            b'+' => {
                self.pos += 1;
                if self.eat(b'+') {
                    Token::new(TokenKind::PlusPlus, Span::new(start, self.pos))
                } else if self.eat(b'=') {
                    Token::new(TokenKind::PlusEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Plus, Span::new(start, self.pos))
                }
            }
            b'-' => {
                self.pos += 1;
                if self.eat(b'-') {
                    Token::new(TokenKind::MinusMinus, Span::new(start, self.pos))
                } else if self.eat(b'=') {
                    Token::new(TokenKind::MinusEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Minus, Span::new(start, self.pos))
                }
            }
            b'*' => {
                self.pos += 1;
                if self.eat(b'=') {
                    Token::new(TokenKind::StarEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Star, Span::new(start, self.pos))
                }
            }
            b'%' => {
                self.pos += 1;
                if self.eat(b'=') {
                    Token::new(TokenKind::PercentEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Percent, Span::new(start, self.pos))
                }
            }
            b'=' => {
                self.pos += 1;
                if self.eat(b'=') {
                    Token::new(TokenKind::EqEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Eq, Span::new(start, self.pos))
                }
            }
            b'!' => {
                self.pos += 1;
                if self.eat(b'=') {
                    Token::new(TokenKind::BangEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Bang, Span::new(start, self.pos))
                }
            }
            b'<' => {
                self.pos += 1;
                if self.eat(b'<') {
                    Token::new(TokenKind::LtLt, Span::new(start, self.pos))
                } else if self.eat(b'=') {
                    Token::new(TokenKind::LtEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Lt, Span::new(start, self.pos))
                }
            }
            b'>' => {
                self.pos += 1;
                if self.eat(b'>') {
                    Token::new(TokenKind::GtGt, Span::new(start, self.pos))
                } else if self.eat(b'=') {
                    Token::new(TokenKind::GtEq, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Gt, Span::new(start, self.pos))
                }
            }
            b'&' => {
                self.pos += 1;
                if self.eat(b'&') {
                    Token::new(TokenKind::AmpAmp, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Amp, Span::new(start, self.pos))
                }
            }
            b'|' => {
                self.pos += 1;
                if self.eat(b'|') {
                    Token::new(TokenKind::PipePipe, Span::new(start, self.pos))
                } else {
                    Token::new(TokenKind::Pipe, Span::new(start, self.pos))
                }
            }
            b'^' => {
                self.pos += 1;
                Token::new(TokenKind::Caret, Span::new(start, self.pos))
            }
            b'~' => {
                self.pos += 1;
                Token::new(TokenKind::Tilde, Span::new(start, self.pos))
            }
            b'?' => {
                self.pos += 1;
                Token::new(TokenKind::Question, Span::new(start, self.pos))
            }
            b':' => {
                self.pos += 1;
                Token::new(TokenKind::Colon, Span::new(start, self.pos))
            }

            // Delimiters
            b'(' => {
                self.pos += 1;
                Token::new(TokenKind::LParen, Span::new(start, self.pos))
            }
            b')' => {
                self.pos += 1;
                Token::new(TokenKind::RParen, Span::new(start, self.pos))
            }
            b'{' => {
                self.pos += 1;
                Token::new(TokenKind::LBrace, Span::new(start, self.pos))
            }
            b'}' => {
                self.pos += 1;
                Token::new(TokenKind::RBrace, Span::new(start, self.pos))
            }
            b'[' => {
                self.pos += 1;
                Token::new(TokenKind::LBracket, Span::new(start, self.pos))
            }
            b']' => {
                self.pos += 1;
                Token::new(TokenKind::RBracket, Span::new(start, self.pos))
            }
            b';' => {
                self.pos += 1;
                Token::new(TokenKind::Semi, Span::new(start, self.pos))
            }
            b',' => {
                self.pos += 1;
                Token::new(TokenKind::Comma, Span::new(start, self.pos))
            }

            // Unknown character
            _ => {
                self.pos += 1;
                Token::new(TokenKind::Error, Span::new(start, self.pos))
            }
        }
    }

    // === Scanning helpers ===

    fn scan_whitespace(&mut self, start: usize) -> Token {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
        Token::new(TokenKind::Whitespace, Span::new(start, self.pos))
    }

    fn scan_line_comment(&mut self, start: usize) -> Token {
        // Skip //
        self.pos += 2;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        Token::new(TokenKind::LineComment, Span::new(start, self.pos))
    }

    fn scan_block_comment(&mut self, start: usize) -> Token {
        // Skip /*
        self.pos += 2;
        let mut depth = 1u32;
        while self.pos + 1 < self.bytes.len() && depth > 0 {
            if self.bytes[self.pos] == b'/' && self.bytes[self.pos + 1] == b'*' {
                // NWScript doesn't support nested comments, but handle gracefully
                depth += 1;
                self.pos += 2;
            } else if self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/' {
                depth -= 1;
                self.pos += 2;
            } else {
                self.pos += 1;
            }
        }
        // If we ran out of input without closing, consume what's left
        if depth > 0 {
            self.pos = self.bytes.len();
        }
        Token::new(TokenKind::BlockComment, Span::new(start, self.pos))
    }

    fn scan_preprocessor(&mut self, start: usize) -> Token {
        // Skip '#'
        self.pos += 1;

        // Skip whitespace between # and directive name
        while self.pos < self.bytes.len() && (self.bytes[self.pos] == b' ' || self.bytes[self.pos] == b'\t') {
            self.pos += 1;
        }

        // Read directive name
        let name_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_alphanumeric() {
            self.pos += 1;
        }

        let name = &self.source[name_start..self.pos];
        match name {
            "include" => Token::new(TokenKind::HashInclude, Span::new(start, self.pos)),
            // Unknown preprocessor directives: emit as error
            _ => Token::new(TokenKind::Error, Span::new(start, self.pos)),
        }
    }

    fn scan_string(&mut self, start: usize) -> Token {
        // Skip opening quote
        self.pos += 1;

        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' => {
                    self.pos += 1;
                    return Token::new(TokenKind::StringLiteral, Span::new(start, self.pos));
                }
                b'\\' => {
                    // Escape sequence — skip next char
                    self.pos += 1;
                    if self.pos < self.bytes.len() {
                        self.pos += 1;
                    }
                }
                b'\n' => {
                    // Unterminated string at newline
                    break;
                }
                _ => self.pos += 1,
            }
        }

        // Unterminated string
        Token::new(TokenKind::StringLiteral, Span::new(start, self.pos))
    }

    fn scan_identifier(&mut self, start: usize) -> Token {
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_alphanumeric() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }

        let word = &self.source[start..self.pos];
        let kind = TokenKind::from_keyword(word).unwrap_or(TokenKind::Ident);
        Token::new(kind, Span::new(start, self.pos))
    }

    fn scan_number(&mut self, start: usize) -> Token {
        // Consume integer part
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        // Check for decimal point
        let is_float = if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self.peek_at(1).is_some_and(|b| b.is_ascii_digit() || b == b'f' || b == b'F')
        {
            self.pos += 1; // skip '.'
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            true
        } else if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' && self.pos > start {
            // Trailing dot like "1." — treat as float
            let next = self.peek_at(1);
            if next.is_none() || !next.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.') {
                self.pos += 1;
                true
            } else {
                false
            }
        } else {
            false
        };

        // Check for 'f' suffix (e.g., 1.0f)
        if self.pos < self.bytes.len() && (self.bytes[self.pos] == b'f' || self.bytes[self.pos] == b'F') {
            self.pos += 1;
            return Token::new(TokenKind::FloatLiteral, Span::new(start, self.pos));
        }

        let kind = if is_float {
            TokenKind::FloatLiteral
        } else {
            TokenKind::IntLiteral
        };
        Token::new(kind, Span::new(start, self.pos))
    }

    fn scan_hex_number(&mut self, start: usize) -> Token {
        // Skip 0x
        self.pos += 2;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_hexdigit() {
            self.pos += 1;
        }
        Token::new(TokenKind::HexLiteral, Span::new(start, self.pos))
    }

    // === Low-level helpers ===

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    /// Consume the next byte if it matches, return whether it did.
    fn eat(&mut self, expected: u8) -> bool {
        if self.pos < self.bytes.len() && self.bytes[self.pos] == expected {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<(TokenKind, &str)> {
        Lexer::tokenize(src)
            .into_iter()
            .filter(|t| !t.is_trivia() && t.kind != TokenKind::Eof)
            .map(|t| (t.kind, t.text(src)))
            .collect()
    }

    #[test]
    fn keywords() {
        let tokens = lex("void int float string object struct");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::KwVoid, "void"),
                (TokenKind::KwInt, "int"),
                (TokenKind::KwFloat, "float"),
                (TokenKind::KwString, "string"),
                (TokenKind::KwObject, "object"),
                (TokenKind::KwStruct, "struct"),
            ]
        );
    }

    #[test]
    fn identifiers() {
        let tokens = lex("foo _bar Baz123");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::Ident, "foo"),
                (TokenKind::Ident, "_bar"),
                (TokenKind::Ident, "Baz123"),
            ]
        );
    }

    #[test]
    fn numbers() {
        let tokens = lex("42 3.14 0xFF .5 1.0f");
        assert_eq!(
            tokens,
            vec![
                (TokenKind::IntLiteral, "42"),
                (TokenKind::FloatLiteral, "3.14"),
                (TokenKind::HexLiteral, "0xFF"),
                (TokenKind::FloatLiteral, ".5"),
                (TokenKind::FloatLiteral, "1.0f"),
            ]
        );
    }

    #[test]
    fn strings() {
        let tokens = lex(r#""hello" "esc\"ape" r"raw" h"hash""#);
        assert_eq!(
            tokens,
            vec![
                (TokenKind::StringLiteral, r#""hello""#),
                (TokenKind::StringLiteral, r#""esc\"ape""#),
                (TokenKind::RawStringLiteral, r#"r"raw""#),
                (TokenKind::HashStringLiteral, r#"h"hash""#),
            ]
        );
    }

    #[test]
    fn operators() {
        let tokens = lex("+ - * / % ++ -- += == != <= >= && || << >>");
        let kinds: Vec<TokenKind> = tokens.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::PlusPlus,
                TokenKind::MinusMinus,
                TokenKind::PlusEq,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::AmpAmp,
                TokenKind::PipePipe,
                TokenKind::LtLt,
                TokenKind::GtGt,
            ]
        );
    }

    #[test]
    fn include_directive() {
        let tokens = lex(r#"#include "nwnx_player""#);
        assert_eq!(
            tokens,
            vec![
                (TokenKind::HashInclude, "#include"),
                (TokenKind::StringLiteral, r#""nwnx_player""#),
            ]
        );
    }

    #[test]
    fn function_declaration() {
        let tokens = lex("void main() { int x = 42; }");
        let kinds: Vec<TokenKind> = tokens.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::KwVoid,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::KwInt,
                TokenKind::Ident,
                TokenKind::Eq,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::RBrace,
            ]
        );
    }

    #[test]
    fn comments_preserved() {
        let all_tokens = Lexer::tokenize("// line\n/* block */\nfoo");
        let kinds: Vec<TokenKind> = all_tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LineComment,
                TokenKind::Newline,
                TokenKind::BlockComment,
                TokenKind::Newline,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }
}
