mod printer;

use crate::{Lexer, Parser};

/// Configuration for the NWScript formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Number of spaces per indent level. Default: 4
    pub indent_width: usize,
    /// Maximum line width before wrapping. Default: 120
    pub max_line_width: usize,
    /// Brace placement style. Default: NextLine (Allman)
    pub brace_style: BraceStyle,
    /// Sort `#include` directives alphabetically. Default: true
    pub sort_includes: bool,
    /// Maximum consecutive blank lines allowed. Default: 1
    pub max_blank_lines: usize,
    /// Remove trailing whitespace from lines. Default: true
    pub trim_trailing_whitespace: bool,
    /// Space between keyword and `(` — `if (x)` vs `if(x)`. Default: true
    pub space_after_keywords: bool,
    /// Spaces inside parentheses — `( x )` vs `(x)`. Default: false
    pub space_inside_parens: bool,
    /// Spaces around binary operators — `a + b` vs `a+b`. Default: true
    pub space_around_operators: bool,
    /// Space after commas — `f(a, b)` vs `f(a,b)`. Default: true
    pub space_after_comma: bool,
}

/// Brace placement style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraceStyle {
    /// Opening brace on its own line (Allman style).
    /// ```text
    /// if (x)
    /// {
    ///     ...
    /// }
    /// ```
    NextLine,
    /// Opening brace on the same line as the statement (K&R style).
    /// ```text
    /// if (x) {
    ///     ...
    /// }
    /// ```
    SameLine,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_width: 4,
            max_line_width: 120,
            brace_style: BraceStyle::NextLine,
            sort_includes: true,
            max_blank_lines: 1,
            trim_trailing_whitespace: true,
            space_after_keywords: true,
            space_inside_parens: false,
            space_around_operators: true,
            space_after_comma: true,
        }
    }
}

/// Format an entire NWScript source file.
pub fn format(source: &str, config: &FormatConfig) -> String {
    let tokens = Lexer::tokenize(source);
    let parsed = Parser::parse(source, tokens.clone());
    let mut result = printer::Printer::new(source, &tokens, &parsed, config).print();
    if config.trim_trailing_whitespace {
        result = trim_trailing_whitespace(&result);
    }
    result
}

/// Trim trailing whitespace from each line.
fn trim_trailing_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    // Preserve final newline if present
    if s.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
