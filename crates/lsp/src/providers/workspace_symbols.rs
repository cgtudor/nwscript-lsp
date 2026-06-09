use tower_lsp::lsp_types::{Location, Range, SymbolInformation, SymbolKind};

use crate::index::WorkspaceIndex;

/// Maximum number of results to return for a workspace symbol query.
const MAX_RESULTS: usize = 200;

/// Search all workspace symbols matching the given query string.
///
/// Uses case-insensitive substring matching (empty query returns nothing to
/// avoid flooding the client with thousands of results).
#[allow(deprecated)] // SymbolInformation.deprecated field
pub fn workspace_symbols(
    index: &WorkspaceIndex,
    query: &str,
) -> Vec<SymbolInformation> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let all = index.all_workspace_symbols();

    let mut results: Vec<SymbolInformation> = all
        .iter()
        .filter(|sym| sym.name.to_lowercase().contains(&query_lower))
        .filter_map(|sym| {
            let file = index.get_file(&sym.uri)?;
            let range = span_to_range(sym.span, &file.line_index);

            Some(SymbolInformation {
                name: sym.name.clone(),
                kind: convert_kind(sym.kind),
                tags: None,
                deprecated: None,
                location: Location {
                    uri: sym.uri.clone(),
                    range,
                },
                container_name: Some(sym.include_name.clone()),
            })
        })
        .take(MAX_RESULTS)
        .collect();

    // Sort: exact prefix matches first, then by name length (shorter = more relevant)
    results.sort_by(|a, b| {
        let a_prefix = a.name.to_lowercase().starts_with(&query_lower);
        let b_prefix = b.name.to_lowercase().starts_with(&query_lower);
        b_prefix
            .cmp(&a_prefix)
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.name.cmp(&b.name))
    });

    results
}

fn convert_kind(kind: crate::index::SymbolKind) -> SymbolKind {
    match kind {
        crate::index::SymbolKind::Function => SymbolKind::FUNCTION,
        crate::index::SymbolKind::Struct => SymbolKind::STRUCT,
        crate::index::SymbolKind::Constant => SymbolKind::CONSTANT,
        crate::index::SymbolKind::Variable => SymbolKind::VARIABLE,
        crate::index::SymbolKind::StructField => SymbolKind::FIELD,
    }
}

fn span_to_range(
    span: nwscript_parser::Span,
    line_index: &nwscript_parser::LineIndex,
) -> Range {
    let (sl, sc) = line_index.line_col(span.start);
    let (el, ec) = line_index.line_col(span.end);
    Range::new(
        tower_lsp::lsp_types::Position::new(sl, sc),
        tower_lsp::lsp_types::Position::new(el, ec),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::WorkspaceIndex;
    use tower_lsp::lsp_types::Url;

    fn make_index(source: &str) -> WorkspaceIndex {
        let index = WorkspaceIndex::new(vec![], vec![], vec![]);
        let uri = Url::parse("file:///test.nss").unwrap();
        index.update_file(&uri, source.to_string());
        index
    }

    #[test]
    fn empty_query_returns_nothing() {
        let index = make_index("void Foo() {}");
        let results = workspace_symbols(&index, "");
        assert!(results.is_empty());
    }

    #[test]
    fn exact_match() {
        let index = make_index("void Foo() {}\nvoid Bar() {}");
        let results = workspace_symbols(&index, "Foo");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Foo");
        assert_eq!(results[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn case_insensitive() {
        let index = make_index("void MyFunction() {}");
        let results = workspace_symbols(&index, "myfunction");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "MyFunction");
    }

    #[test]
    fn substring_match() {
        let index = make_index("void GetLocalInt() {}\nvoid SetLocalInt() {}");
        let results = workspace_symbols(&index, "LocalInt");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn prefix_matches_sorted_first() {
        let index = make_index("void GetFoo() {}\nvoid FooGet() {}");
        let results = workspace_symbols(&index, "Foo");
        assert_eq!(results.len(), 2);
        // FooGet starts with "Foo", so it should come first
        assert_eq!(results[0].name, "FooGet");
    }

    #[test]
    fn includes_structs_and_constants() {
        let index = make_index("struct MyStruct { int x; };\nconst int MY_CONST = 42;");
        let results = workspace_symbols(&index, "My");
        assert_eq!(results.len(), 2);
        let kinds: Vec<_> = results.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&SymbolKind::STRUCT));
        assert!(kinds.contains(&SymbolKind::CONSTANT));
    }
}
