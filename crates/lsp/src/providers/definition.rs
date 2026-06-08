use crate::index::{SymbolInfo, WorkspaceIndex};
use nwscript_parser::LineIndex;
use tower_lsp::lsp_types::{Location, Position, Url};

use super::symbols::span_to_range;

/// Find the definition of the symbol at the given position, searching across files.
pub fn goto_definition(
    index: &WorkspaceIndex,
    source: &str,
    line_index: &LineIndex,
    position: Position,
    uri: &Url,
) -> Option<Location> {
    let offset = line_index.offset(position.line, position.character)?;
    let target_name = super::hover::find_ident_at(source, offset as usize)?;

    // Search visible symbols (own file + transitive includes)
    let symbols = index.visible_symbols(uri);

    // Prefer function definitions over prototypes
    let mut best: Option<&SymbolInfo> = None;
    for sym in &symbols {
        if sym.name == target_name {
            match best {
                None => best = Some(sym),
                Some(prev) => {
                    // Prefer definitions (longer detail = has body)
                    if prev.detail.ends_with(" {...}") || !sym.detail.ends_with(" {...}") {
                        // Keep prev if it's already a definition
                    } else {
                        best = Some(sym);
                    }
                }
            }
        }
    }

    let sym = best?;

    // Need the target file's line index to convert spans
    let target_file = index.get_file(&sym.uri)?;
    let range = span_to_range(sym.span, &target_file.line_index);

    Some(Location {
        uri: sym.uri.clone(),
        range,
    })
}
