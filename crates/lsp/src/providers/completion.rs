use nwscript_parser::{Declaration, ParsedFile};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat,
};

/// Gather completion items from a parsed file.
pub fn completions_from_file(parsed: &ParsedFile) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            Declaration::Function(f) => {
                if let Some(name) = &f.name {
                    let params: Vec<String> = f
                        .params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let pname = p
                                .name
                                .as_ref()
                                .map(|n| n.name.as_str())
                                .unwrap_or("arg");
                            format!("${{{}: {pname}}}", i + 1)
                        })
                        .collect();

                    let snippet = if f.params.is_empty() {
                        format!("{}()", name.name)
                    } else {
                        format!("{}({})", name.name, params.join(", "))
                    };

                    let detail = super::symbols::format_function_signature(f);

                    items.push(CompletionItem {
                        label: name.name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(detail),
                        insert_text: Some(snippet),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    });
                }
            }
            Declaration::Struct(s) => {
                if let Some(name) = &s.name {
                    items.push(CompletionItem {
                        label: name.name.clone(),
                        kind: Some(CompletionItemKind::STRUCT),
                        detail: Some("struct".into()),
                        ..Default::default()
                    });
                }
            }
            Declaration::GlobalVar(v) => {
                if let Some(name) = &v.name {
                    let kind = if v.is_const {
                        CompletionItemKind::CONSTANT
                    } else {
                        CompletionItemKind::VARIABLE
                    };
                    items.push(CompletionItem {
                        label: name.name.clone(),
                        kind: Some(kind),
                        detail: Some(super::symbols::format_type(&v.ty.kind)),
                        ..Default::default()
                    });
                }
            }
            Declaration::Include(_) => {}
        }
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
