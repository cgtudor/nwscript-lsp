use nwscript_parser::{Declaration, FunctionDecl, LineIndex, StructDecl, TypeKind, VarDecl};
use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

/// Extract document symbols from a parsed file.
pub fn document_symbols(
    parsed: &nwscript_parser::ParsedFile,
    line_index: &LineIndex,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    for decl in &parsed.declarations {
        if let Some(sym) = decl_to_symbol(decl, line_index) {
            symbols.push(sym);
        }
    }

    symbols
}

fn decl_to_symbol(decl: &Declaration, li: &LineIndex) -> Option<DocumentSymbol> {
    match decl {
        Declaration::Function(f) => func_symbol(f, li),
        Declaration::Struct(s) => struct_symbol(s, li),
        Declaration::GlobalVar(v) => var_symbol(v, li),
        Declaration::Include(_) => None,
    }
}

fn func_symbol(f: &FunctionDecl, li: &LineIndex) -> Option<DocumentSymbol> {
    let name = f.name.as_ref()?.name.clone();
    let range = span_to_range(f.span, li);
    let selection = span_to_range(f.name.as_ref()?.span, li);

    let detail = format_function_signature(f);

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: Some(detail),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        range,
        selection_range: selection,
        children: None,
    })
}

fn struct_symbol(s: &StructDecl, li: &LineIndex) -> Option<DocumentSymbol> {
    let name = s.name.as_ref()?.name.clone();
    let range = span_to_range(s.span, li);
    let selection = span_to_range(s.name.as_ref()?.span, li);

    let children: Vec<DocumentSymbol> = s
        .fields
        .iter()
        .filter_map(|field| {
            let field_name = field.name.as_ref()?.name.clone();
            let field_range = span_to_range(field.span, li);
            let field_sel = span_to_range(field.name.as_ref()?.span, li);
            #[allow(deprecated)]
            Some(DocumentSymbol {
                name: field_name,
                detail: Some(format_type(&field.ty.kind)),
                kind: SymbolKind::FIELD,
                tags: None,
                deprecated: None,
                range: field_range,
                selection_range: field_sel,
                children: None,
            })
        })
        .collect();

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: Some("struct".into()),
        kind: SymbolKind::STRUCT,
        tags: None,
        deprecated: None,
        range,
        selection_range: selection,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    })
}

fn var_symbol(v: &VarDecl, li: &LineIndex) -> Option<DocumentSymbol> {
    let name = v.name.as_ref()?.name.clone();
    let range = span_to_range(v.span, li);
    let selection = span_to_range(v.name.as_ref()?.span, li);

    let kind = if v.is_const {
        SymbolKind::CONSTANT
    } else {
        SymbolKind::VARIABLE
    };

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: Some(format_type(&v.ty.kind)),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: selection,
        children: None,
    })
}

pub fn format_function_signature(f: &FunctionDecl) -> String {
    let ret = format_type(&f.return_type.kind);
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let ty = format_type(&p.ty.kind);
            match &p.name {
                Some(n) => format!("{ty} {}", n.name),
                None => ty,
            }
        })
        .collect();
    format!("{ret}({}){}",
        params.join(", "),
        if f.is_prototype() { "" } else { " {...}" }
    )
}

pub fn format_type(kind: &TypeKind) -> String {
    match kind {
        TypeKind::Void => "void".into(),
        TypeKind::Int => "int".into(),
        TypeKind::Float => "float".into(),
        TypeKind::String => "string".into(),
        TypeKind::Object => "object".into(),
        TypeKind::Vector => "vector".into(),
        TypeKind::Action => "action".into(),
        TypeKind::Effect => "effect".into(),
        TypeKind::Event => "event".into(),
        TypeKind::ItemProperty => "itemproperty".into(),
        TypeKind::Location => "location".into(),
        TypeKind::Talent => "talent".into(),
        TypeKind::Json => "json".into(),
        TypeKind::SqlQuery => "sqlquery".into(),
        TypeKind::Cassowary => "cassowary".into(),
        TypeKind::Struct(name) => format!("struct {name}"),
        TypeKind::Error => "<error>".into(),
    }
}

pub fn span_to_range(span: nwscript_parser::Span, li: &LineIndex) -> Range {
    let (sl, sc) = li.line_col(span.start);
    let (el, ec) = li.line_col(span.end);
    Range::new(Position::new(sl, sc), Position::new(el, ec))
}
