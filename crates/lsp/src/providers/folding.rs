use nwscript_parser::ast::*;
use nwscript_parser::{LineIndex, ParsedFile, TokenKind};
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

/// Compute folding ranges for a file.
///
/// Produces ranges for:
/// - Function bodies
/// - Struct bodies
/// - Block statements (if/while/for/switch/do-while bodies)
/// - Consecutive `#include` groups (collapsed as imports)
/// - Block comments (`/* ... */`)
/// - Consecutive line comment groups (`//` on adjacent lines)
pub fn folding_ranges(parsed: &ParsedFile, source: &str, line_index: &LineIndex) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();

    // AST-based ranges
    collect_include_ranges(&parsed.declarations, line_index, &mut ranges);
    for decl in &parsed.declarations {
        collect_decl_ranges(decl, line_index, &mut ranges);
    }

    // Token-based ranges for comments
    collect_comment_ranges(source, line_index, &mut ranges);

    ranges
}

/// Fold consecutive `#include` directives into a single "imports" range.
fn collect_include_ranges(
    decls: &[Declaration],
    li: &LineIndex,
    ranges: &mut Vec<FoldingRange>,
) {
    let mut i = 0;
    while i < decls.len() {
        if matches!(&decls[i], Declaration::Include(_)) {
            let start = i;
            while i < decls.len() && matches!(&decls[i], Declaration::Include(_)) {
                i += 1;
            }
            // Only fold if there are 2+ includes
            if i - start >= 2 {
                let first_span = decls[start].span();
                let last_span = decls[i - 1].span();
                let (start_line, _) = li.line_col(first_span.start);
                let (end_line, _) = li.line_col(last_span.end);
                if end_line > start_line {
                    ranges.push(FoldingRange {
                        start_line,
                        start_character: None,
                        end_line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Imports),
                        collapsed_text: None,
                    });
                }
            }
        } else {
            i += 1;
        }
    }
}

fn collect_decl_ranges(decl: &Declaration, li: &LineIndex, ranges: &mut Vec<FoldingRange>) {
    match decl {
        Declaration::Function(f) => {
            // Fold the entire function (from signature to closing brace)
            if f.body.is_some() {
                push_span_range(f.span, li, ranges);
            }
            // Also recurse into the body for nested blocks
            if let Some(body) = &f.body {
                collect_block_ranges(body, li, ranges);
            }
        }
        Declaration::Struct(s) => {
            push_span_range(s.span, li, ranges);
        }
        _ => {}
    }
}

fn collect_block_ranges(block: &Block, li: &LineIndex, ranges: &mut Vec<FoldingRange>) {
    for stmt in &block.stmts {
        collect_stmt_ranges(stmt, li, ranges);
    }
}

fn collect_stmt_ranges(stmt: &Stmt, li: &LineIndex, ranges: &mut Vec<FoldingRange>) {
    match stmt {
        Stmt::If(s) => {
            push_span_range(s.span, li, ranges);
            if let Stmt::Block(b) = s.then_branch.as_ref() {
                collect_block_ranges(b, li, ranges);
            }
            if let Some(else_branch) = &s.else_branch {
                if let Stmt::Block(b) = else_branch.as_ref() {
                    collect_block_ranges(b, li, ranges);
                }
                collect_stmt_ranges(else_branch, li, ranges);
            }
        }
        Stmt::While(s) => {
            push_span_range(s.span, li, ranges);
            if let Stmt::Block(b) = s.body.as_ref() {
                collect_block_ranges(b, li, ranges);
            }
        }
        Stmt::DoWhile(s) => {
            push_span_range(s.span, li, ranges);
            if let Stmt::Block(b) = s.body.as_ref() {
                collect_block_ranges(b, li, ranges);
            }
        }
        Stmt::For(s) => {
            push_span_range(s.span, li, ranges);
            if let Stmt::Block(b) = s.body.as_ref() {
                collect_block_ranges(b, li, ranges);
            }
        }
        Stmt::Switch(s) => {
            push_span_range(s.span, li, ranges);
            for case in &s.cases {
                for case_stmt in &case.stmts {
                    collect_stmt_ranges(case_stmt, li, ranges);
                }
            }
        }
        Stmt::Block(b) => {
            push_span_range(b.span, li, ranges);
            collect_block_ranges(b, li, ranges);
        }
        _ => {}
    }
}

/// Push a folding range for a span, only if it covers more than one line.
fn push_span_range(span: nwscript_parser::Span, li: &LineIndex, ranges: &mut Vec<FoldingRange>) {
    let (start_line, _) = li.line_col(span.start);
    let (end_line, _) = li.line_col(span.end);
    if end_line > start_line {
        ranges.push(FoldingRange {
            start_line,
            start_character: None,
            end_line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        });
    }
}

/// Fold block comments and groups of consecutive line comments.
fn collect_comment_ranges(source: &str, li: &LineIndex, ranges: &mut Vec<FoldingRange>) {
    let tokens = nwscript_parser::Lexer::tokenize(source);

    let mut i = 0;
    while i < tokens.len() {
        match tokens[i].kind {
            TokenKind::BlockComment => {
                let (start_line, _) = li.line_col(tokens[i].span.start);
                let (end_line, _) = li.line_col(tokens[i].span.end);
                if end_line > start_line {
                    ranges.push(FoldingRange {
                        start_line,
                        start_character: None,
                        end_line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Comment),
                        collapsed_text: None,
                    });
                }
                i += 1;
            }
            TokenKind::LineComment => {
                // Group consecutive line comments (only whitespace/newlines between them)
                let group_start = i;
                let (start_line, _) = li.line_col(tokens[i].span.start);
                let mut end_line = start_line;

                i += 1;
                while i < tokens.len() {
                    // Skip whitespace and newlines between comments
                    if matches!(tokens[i].kind, TokenKind::Whitespace | TokenKind::Newline) {
                        i += 1;
                        continue;
                    }
                    if tokens[i].kind == TokenKind::LineComment {
                        let (comment_line, _) = li.line_col(tokens[i].span.start);
                        // Only group if on the very next line after previous comment
                        if comment_line == end_line + 1 {
                            end_line = comment_line;
                            i += 1;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // Only fold groups of 2+ consecutive line comments
                let _ = group_start; // used for the counting logic above
                if end_line > start_line {
                    ranges.push(FoldingRange {
                        start_line,
                        start_character: None,
                        end_line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Comment),
                        collapsed_text: None,
                    });
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(source: &str) -> Vec<FoldingRange> {
        let parsed = nwscript_parser::parse(source);
        let li = LineIndex::new(source);
        folding_ranges(&parsed, source, &li)
    }

    fn lines(range: &FoldingRange) -> (u32, u32) {
        (range.start_line, range.end_line)
    }

    #[test]
    fn function_body_folds() {
        let src = "void Foo()\n{\n    int x = 1;\n}";
        let ranges = fold(src);
        let region_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        assert!(!region_ranges.is_empty());
        assert_eq!(lines(region_ranges[0]), (0, 3));
    }

    #[test]
    fn struct_folds() {
        let src = "struct Foo\n{\n    int x;\n    int y;\n};";
        let ranges = fold(src);
        let region_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        assert!(!region_ranges.is_empty());
        assert_eq!(lines(region_ranges[0]), (0, 4));
    }

    #[test]
    fn include_group_folds() {
        let src = "#include \"a\"\n#include \"b\"\n#include \"c\"";
        let ranges = fold(src);
        let import_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Imports))
            .collect();
        assert_eq!(import_ranges.len(), 1);
        assert_eq!(lines(import_ranges[0]), (0, 2));
    }

    #[test]
    fn single_include_no_fold() {
        let src = "#include \"a\"";
        let ranges = fold(src);
        let import_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Imports))
            .collect();
        assert!(import_ranges.is_empty());
    }

    #[test]
    fn block_comment_folds() {
        let src = "/* This is\n   a multi-line\n   comment */\nvoid Foo() {}";
        let ranges = fold(src);
        let comment_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Comment))
            .collect();
        assert_eq!(comment_ranges.len(), 1);
        assert_eq!(lines(comment_ranges[0]), (0, 2));
    }

    #[test]
    fn consecutive_line_comments_fold() {
        let src = "// line 1\n// line 2\n// line 3\nvoid Foo() {}";
        let ranges = fold(src);
        let comment_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Comment))
            .collect();
        assert_eq!(comment_ranges.len(), 1);
        assert_eq!(lines(comment_ranges[0]), (0, 2));
    }

    #[test]
    fn single_line_comment_no_fold() {
        let src = "// just one comment\nvoid Foo() {}";
        let ranges = fold(src);
        let comment_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Comment))
            .collect();
        assert!(comment_ranges.is_empty());
    }

    #[test]
    fn if_else_folds() {
        let src = "void Foo()\n{\n    if (1)\n    {\n        int x;\n    }\n    else\n    {\n        int y;\n    }\n}";
        let ranges = fold(src);
        let region_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        // Function body + if/else statement
        assert!(region_ranges.len() >= 2);
    }

    #[test]
    fn single_line_no_fold() {
        let src = "void Foo() { int x = 1; }";
        let ranges = fold(src);
        let region_ranges: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        assert!(region_ranges.is_empty());
    }
}
