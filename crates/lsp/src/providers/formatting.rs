use nwscript_parser::formatter::{self, BraceStyle, FormatConfig};
use tower_lsp::lsp_types::*;

/// Build a `FormatConfig` from LSP formatting options and our custom settings.
pub fn build_config(
    options: &FormattingOptions,
    custom: &FormatterSettings,
) -> FormatConfig {
    let mut config = FormatConfig::default();

    // LSP standard: tab size
    config.indent_width = options.tab_size as usize;

    // Custom settings override defaults
    if let Some(w) = custom.max_line_width {
        config.max_line_width = w;
    }
    if let Some(style) = &custom.brace_style {
        config.brace_style = match style.as_str() {
            "sameLine" => BraceStyle::SameLine,
            _ => BraceStyle::NextLine,
        };
    }
    if let Some(v) = custom.sort_includes {
        config.sort_includes = v;
    }
    if let Some(v) = custom.max_blank_lines {
        config.max_blank_lines = v;
    }
    if let Some(v) = custom.trim_trailing_whitespace {
        config.trim_trailing_whitespace = v;
    }
    if let Some(v) = custom.space_after_keywords {
        config.space_after_keywords = v;
    }
    if let Some(v) = custom.space_inside_parens {
        config.space_inside_parens = v;
    }
    if let Some(v) = custom.space_around_operators {
        config.space_around_operators = v;
    }
    if let Some(v) = custom.space_after_comma {
        config.space_after_comma = v;
    }

    config
}

/// Format an entire document and return a single text edit replacing all content.
pub fn format_document(source: &str, config: &FormatConfig) -> Vec<TextEdit> {
    let formatted = formatter::format(source, config);

    if formatted == source {
        return Vec::new();
    }

    // Count lines in source for the range
    let line_count = source.lines().count() as u32;
    let last_line_len = source.lines().last().map_or(0, |l| l.len()) as u32;

    vec![TextEdit {
        range: Range::new(
            Position::new(0, 0),
            Position::new(line_count, last_line_len),
        ),
        new_text: formatted,
    }]
}

/// On-type formatting: format only the affected region near the cursor.
///
/// For `;`: formats the current statement (scans back to find statement start).
/// For `}`: formats the block that was just closed (scans back to matching `{`).
/// For `\n`: provides correct indentation for the new line.
pub fn on_type_format(
    source: &str,
    position: Position,
    ch: &str,
    config: &FormatConfig,
) -> Vec<TextEdit> {
    let lines: Vec<&str> = source.lines().collect();
    let line = position.line as usize;

    match ch {
        ";" => {
            if line >= lines.len() {
                return Vec::new();
            }
            let formatted = formatter::format(source, config);
            if formatted == source {
                return Vec::new();
            }
            // Only emit edits for the current statement region
            let stmt_start = find_statement_start(&lines, line);
            compute_line_edits_in_range(source, &formatted, stmt_start, line)
        }

        "}" => {
            if line >= lines.len() {
                return Vec::new();
            }
            let formatted = formatter::format(source, config);
            if formatted == source {
                return Vec::new();
            }
            // Only emit edits for the block that was just closed
            let block_start = find_matching_open_brace(&lines, line);
            compute_line_edits_in_range(source, &formatted, block_start, line)
        }

        "\n" => {
            if line == 0 || line >= lines.len() {
                return Vec::new();
            }

            let indent_level = compute_indent_level(&lines, line);
            let new_indent = " ".repeat(indent_level * config.indent_width);

            let current_line = lines.get(line).unwrap_or(&"");
            if current_line.trim().is_empty() {
                vec![TextEdit {
                    range: Range::new(
                        Position::new(line as u32, 0),
                        Position::new(line as u32, current_line.len() as u32),
                    ),
                    new_text: new_indent,
                }]
            } else {
                Vec::new()
            }
        }

        _ => Vec::new(),
    }
}

/// Scan backwards from cursor to find the start of the current statement.
/// A statement starts after a line ending with `{`, `}`, `;`, `:`, or an empty line.
fn find_statement_start(lines: &[&str], cursor_line: usize) -> usize {
    for i in (0..cursor_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with('{')
            || trimmed.ends_with('}')
            || trimmed.ends_with(';')
            || trimmed.ends_with(':')
            || trimmed.is_empty()
        {
            return i + 1;
        }
    }
    0
}

/// Scan backwards from a `}` line to find the matching `{` line.
fn find_matching_open_brace(lines: &[&str], close_line: usize) -> usize {
    let mut depth: i32 = 0;
    for i in (0..=close_line).rev() {
        let trimmed = lines[i].trim();
        // Simple brace counting (skip comment lines)
        if trimmed.starts_with("//") {
            continue;
        }
        for ch in trimmed.chars().rev() {
            match ch {
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        // Include the line before `{` (the statement header: if/for/func etc.)
                        return if i > 0 { i - 1 } else { 0 };
                    }
                }
                _ => {}
            }
        }
    }
    0
}

/// Diff two document versions and return edits only for lines within [range_start, range_end].
///
/// Safety: if the formatter changed the total line count, the line-by-line mapping
/// is unreliable (lines shift), so we return no edits. Format-on-save will handle
/// those cases with a full document replacement instead.
fn compute_line_edits_in_range(
    old: &str,
    new: &str,
    range_start: usize,
    range_end: usize,
) -> Vec<TextEdit> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Only safe when line count is unchanged — otherwise lines have shifted
    // and index-based comparison would produce wrong edits (content deletion).
    if old_lines.len() != new_lines.len() {
        return Vec::new();
    }

    let mut edits = Vec::new();
    let end = range_end.min(old_lines.len().saturating_sub(1));

    for i in range_start..=end {
        if old_lines[i] != new_lines[i] {
            edits.push(TextEdit {
                range: Range::new(
                    Position::new(i as u32, 0),
                    Position::new(i as u32, old_lines[i].len() as u32),
                ),
                new_text: new_lines[i].to_string(),
            });
        }
    }

    edits
}

/// Compute the expected indent level at a given line by counting brace balance above.
fn compute_indent_level(lines: &[&str], target_line: usize) -> usize {
    let mut depth: i32 = 0;
    for line in &lines[..target_line] {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                '/' if trimmed.contains("//") => break,
                _ => {}
            }
        }
    }
    depth.max(0) as usize
}

/// Custom formatter settings from VS Code configuration.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatterSettings {
    pub max_line_width: Option<usize>,
    pub brace_style: Option<String>,
    pub sort_includes: Option<bool>,
    pub max_blank_lines: Option<usize>,
    pub trim_trailing_whitespace: Option<bool>,
    pub space_after_keywords: Option<bool>,
    pub space_inside_parens: Option<bool>,
    pub space_around_operators: Option<bool>,
    pub space_after_comma: Option<bool>,
}
