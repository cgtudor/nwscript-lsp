use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// Runs `nwn_script_comp` on a file and parses the output into LSP diagnostics.
pub async fn compile_file(
    compiler_path: &Path,
    file_path: &Path,
    include_dirs: &[PathBuf],
) -> Vec<Diagnostic> {
    let mut cmd = Command::new(compiler_path);

    // Add include directories as -I flags
    for dir in include_dirs {
        cmd.arg("-I").arg(dir);
    }

    cmd.arg(file_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to run compiler: {e}");
            return vec![Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("nwscript-lsp".into()),
                message: format!("Could not run compiler: {e}"),
                ..Default::default()
            }];
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    parse_compiler_output(&combined)
}

/// Parse nwn_script_comp output into diagnostics.
///
/// The compiler outputs errors/warnings in formats like:
///   `filename.nss:42: Error: NSC6012: Undeclared identifier "foo".`
///   `filename.nss(42): Error: ...`
fn parse_compiler_output(output: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Try to parse error/warning lines.
        if let Some(diag) = parse_compiler_line(line) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

fn parse_compiler_line(line: &str) -> Option<Diagnostic> {
    // Formats:
    //   file.nss:LINE: Error: MESSAGE
    //   file.nss(LINE): Error: MESSAGE
    //   file.nss:LINE:COL: Error: MESSAGE
    //   Error: MESSAGE (no location)

    // Look for "Error:" or "Warning:" to determine severity
    let (severity, msg_start) = if let Some(idx) = line.find("Error:") {
        (DiagnosticSeverity::ERROR, idx + 6)
    } else if let Some(idx) = line.find("Warning:") {
        (DiagnosticSeverity::WARNING, idx + 8)
    } else {
        return None;
    };

    let message = line[msg_start..].trim().to_string();

    // Try to extract line number
    let line_num = extract_line_number(line).unwrap_or(0);

    Some(Diagnostic {
        range: Range::new(
            Position::new(line_num.saturating_sub(1), 0),
            Position::new(line_num.saturating_sub(1), 1000),
        ),
        severity: Some(severity),
        source: Some("nwn_script_comp".into()),
        message,
        ..Default::default()
    })
}

fn extract_line_number(line: &str) -> Option<u32> {
    // Try `file:LINE:` format
    if let Some(colon1) = line.find(':') {
        let rest = &line[colon1 + 1..];
        if let Some(colon2) = rest.find(':') {
            if let Ok(n) = rest[..colon2].trim().parse::<u32>() {
                return Some(n);
            }
        }
    }

    // Try `file(LINE)` format
    if let Some(paren) = line.find('(') {
        let rest = &line[paren + 1..];
        if let Some(close) = rest.find(')') {
            if let Ok(n) = rest[..close].trim().parse::<u32>() {
                return Some(n);
            }
        }
    }

    None
}
