pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod token;

pub use ast::*;
pub use lexer::Lexer;
pub use parser::Parser;
pub use span::{LineIndex, Span};
pub use token::{Token, TokenKind};

/// Parse a NWScript source file and return the AST with any parse errors.
pub fn parse(source: &str) -> ParsedFile {
    let tokens = Lexer::tokenize(source);
    Parser::parse(source, tokens)
}
