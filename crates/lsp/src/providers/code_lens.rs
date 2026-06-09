use nwscript_parser::{Declaration, LineIndex, ParsedFile};
use tower_lsp::lsp_types::{CodeLens, Command, Position, Range, Url};

use crate::index::WorkspaceIndex;

/// Produce fully-resolved code lenses for functions and structs.
/// Uses batch reference counting to scan the workspace once for all symbols,
/// instead of resolving each lens individually (which would scan N times).
pub fn code_lenses(
    parsed: &ParsedFile,
    line_index: &LineIndex,
    index: &WorkspaceIndex,
    uri: &Url,
) -> Vec<CodeLens> {
    // Phase 1: collect all symbols that need lenses
    struct LensCandidate {
        name: String,
        decl_count: usize,
        range: Range,
    }
    let mut candidates: Vec<LensCandidate> = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            Declaration::Function(f) => {
                let Some(name) = &f.name else { continue };
                if is_entry_point(&name.name) {
                    continue;
                }
                if f.body.is_none() && has_definition_in_file(parsed, &name.name) {
                    continue;
                }

                let decl_count = count_declarations_in_file(parsed, &name.name);
                let (line, col) = line_index.line_col(name.span.start);
                let range = Range::new(Position::new(line, col), Position::new(line, col));
                candidates.push(LensCandidate {
                    name: name.name.clone(),
                    decl_count,
                    range,
                });
            }
            Declaration::Struct(s) => {
                let Some(name) = &s.name else { continue };
                let (line, col) = line_index.line_col(name.span.start);
                let range = Range::new(Position::new(line, col), Position::new(line, col));
                candidates.push(LensCandidate {
                    name: name.name.clone(),
                    decl_count: 1,
                    range,
                });
            }
            _ => {}
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Phase 2: batch count all references in a single workspace scan
    let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
    let counts = super::references::count_references_batch(index, &names);

    // Phase 3: build resolved lenses
    let uri_str = uri.as_str();
    candidates
        .into_iter()
        .map(|c| {
            let total = counts.get(&c.name).copied().unwrap_or(0);
            let ref_count = total.saturating_sub(c.decl_count);

            let title = match ref_count {
                0 => "0 references".to_string(),
                1 => "1 reference".to_string(),
                n => format!("{n} references"),
            };

            CodeLens {
                range: c.range,
                command: Some(Command {
                    title,
                    command: "nwscript-lsp.findReferences".to_string(),
                    arguments: Some(vec![
                        serde_json::json!(uri_str),
                        serde_json::json!({
                            "line": c.range.start.line,
                            "character": c.range.start.character,
                        }),
                    ]),
                }),
                data: None,
            }
        })
        .collect()
}

fn has_definition_in_file(parsed: &ParsedFile, name: &str) -> bool {
    parsed.declarations.iter().any(|d| {
        if let Declaration::Function(f) = d {
            f.body.is_some() && f.name.as_ref().map(|n| n.name.as_str()) == Some(name)
        } else {
            false
        }
    })
}

fn count_declarations_in_file(parsed: &ParsedFile, name: &str) -> usize {
    parsed
        .declarations
        .iter()
        .filter(|d| {
            if let Declaration::Function(f) = d {
                f.name.as_ref().map(|n| n.name.as_str()) == Some(name)
            } else {
                false
            }
        })
        .count()
}

fn is_entry_point(name: &str) -> bool {
    matches!(name, "main" | "StartingConditional")
}
