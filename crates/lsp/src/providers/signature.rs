use crate::index::{SymbolInfo, SymbolKind};
use tower_lsp::lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel,
    SignatureHelp, SignatureInformation,
};

/// Build signature help for a function call at the cursor.
///
/// `text_before_cursor` is the text from the start of the line up to the cursor.
/// We parse it backwards to find the function name and which parameter we're in.
pub fn signature_help(
    text_before_cursor: &str,
    symbols: &[SymbolInfo],
) -> Option<SignatureHelp> {
    let (func_name, active_param) = find_call_context(text_before_cursor)?;

    // Find matching function symbols
    let signatures: Vec<SignatureInformation> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function && s.name == func_name)
        .filter_map(|s| build_signature(s))
        .collect();

    if signatures.is_empty() {
        return None;
    }

    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

fn build_signature(sym: &SymbolInfo) -> Option<SignatureInformation> {
    let params = sym.params.as_ref()?;
    let ret = sym.return_type.as_deref().unwrap_or("void");

    let param_labels: Vec<ParameterInformation> = params
        .iter()
        .map(|p| {
            let label = if p.has_default {
                match &p.default_text {
                    Some(def) => format!("{} {} = {}", p.ty, p.name, def),
                    None => format!("{} {} = ...", p.ty, p.name),
                }
            } else {
                format!("{} {}", p.ty, p.name)
            };
            ParameterInformation {
                label: ParameterLabel::Simple(label),
                documentation: None,
            }
        })
        .collect();

    let param_strings: Vec<String> = params
        .iter()
        .map(|p| {
            if p.has_default {
                match &p.default_text {
                    Some(def) => format!("{} {} = {}", p.ty, p.name, def),
                    None => format!("{} {} = ...", p.ty, p.name),
                }
            } else {
                format!("{} {}", p.ty, p.name)
            }
        })
        .collect();

    let label = format!("{} {}({})", ret, sym.name, param_strings.join(", "));

    let documentation = sym.doc.as_ref().map(|doc| {
        Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc.clone(),
        })
    });

    Some(SignatureInformation {
        label,
        documentation,
        parameters: Some(param_labels),
        active_parameter: None,
    })
}

/// Parse backwards from cursor to find the function being called and which
/// parameter the cursor is in.
///
/// Returns (function_name, active_parameter_index).
fn find_call_context(text: &str) -> Option<(String, u32)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut comma_count = 0u32;
    let mut i = bytes.len();

    // Walk backwards to find the matching open paren
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    // Found the opening paren of our call
                    // Now extract the function name before it
                    let before = &text[..i];
                    let func_name = extract_trailing_ident(before)?;
                    return Some((func_name, comma_count));
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                comma_count += 1;
            }
            b'"' => {
                // Skip string backwards
                if i > 0 {
                    i -= 1;
                    while i > 0 && bytes[i] != b'"' {
                        if bytes[i] == b'\\' && i > 0 {
                            i -= 1;
                        }
                        i -= 1;
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// Extract the last identifier from a string.
/// "  GetName" -> "GetName"
/// "foo = Bar" -> "Bar"
fn extract_trailing_ident(text: &str) -> Option<String> {
    let text = text.trim_end();
    let bytes = text.as_bytes();
    let end = bytes.len();

    // Find end of identifier
    if end == 0 || !is_ident_char(bytes[end - 1]) {
        return None;
    }

    // Find start of identifier
    let mut start = end;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    if start == end {
        return None;
    }

    Some(text[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_call_basic() {
        assert_eq!(
            find_call_context("SendMessageToPC(oPC, "),
            Some(("SendMessageToPC".into(), 1))
        );
    }

    #[test]
    fn find_call_first_param() {
        assert_eq!(
            find_call_context("GetName("),
            Some(("GetName".into(), 0))
        );
    }

    #[test]
    fn find_call_nested() {
        assert_eq!(
            find_call_context("SendMessageToPC(oPC, IntToString("),
            Some(("IntToString".into(), 0))
        );
    }

    #[test]
    fn find_call_third_param() {
        assert_eq!(
            find_call_context("Foo(a, b, "),
            Some(("Foo".into(), 2))
        );
    }
}
