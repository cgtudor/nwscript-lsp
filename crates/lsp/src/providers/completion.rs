use crate::index::{SymbolInfo, SymbolKind};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Build completion items from cross-file visible symbols.
pub fn completions_from_symbols(symbols: &[SymbolInfo]) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for sym in symbols {
        // Skip struct fields in general completion (they appear on dot-completion)
        if sym.kind == SymbolKind::StructField {
            continue;
        }

        // Deduplicate (prototypes + definitions produce same name)
        if !seen.insert(&sym.name) {
            continue;
        }

        let item = match sym.kind {
            SymbolKind::Function => {
                let snippet = if let Some(params) = &sym.params {
                    if params.is_empty() {
                        format!("{}()", sym.name)
                    } else {
                        let param_snippets: Vec<String> = params
                            .iter()
                            .enumerate()
                            .map(|(i, p)| format!("${{{}: {}}}", i + 1, p.name))
                            .collect();
                        format!("{}({})", sym.name, param_snippets.join(", "))
                    }
                } else {
                    format!("{}()", sym.name)
                };

                CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(sym.detail.clone()),
                    insert_text: Some(snippet),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                }
            }
            SymbolKind::Struct => CompletionItem {
                label: sym.name.clone(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some(sym.detail.clone()),
                ..Default::default()
            },
            SymbolKind::Constant => CompletionItem {
                label: sym.name.clone(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(sym.detail.clone()),
                ..Default::default()
            },
            SymbolKind::Variable => CompletionItem {
                label: sym.name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(sym.detail.clone()),
                ..Default::default()
            },
            SymbolKind::StructField => continue,
        };

        items.push(item);
    }

    items
}

/// NWScript keyword completions.
pub fn keyword_completions() -> Vec<CompletionItem> {
    let keywords = [
        "void", "int", "float", "string", "object", "struct", "vector",
        "effect", "event", "itemproperty", "location", "talent", "json",
        "action", "sqlquery", "cassowary",
        "if", "else", "while", "for", "do", "switch", "case", "default",
        "break", "continue", "return", "const",
        "TRUE", "FALSE", "OBJECT_SELF", "OBJECT_INVALID",
    ];

    keywords
        .iter()
        .map(|&kw| CompletionItem {
            label: kw.into(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        })
        .collect()
}
