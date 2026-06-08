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

    // Simulate mode: compile but don't write output files
    cmd.arg("-s");
    // No entry point required (works for include files too)
    cmd.arg("-n");
    // Collect all errors, don't stop at first
    cmd.arg("-E");
    // Quiet: suppress info-level logging, only errors
    cmd.arg("--quiet");

    // Build comma-separated directories for --dirs
    if !include_dirs.is_empty() {
        let dirs_str = include_dirs
            .iter()
            .filter_map(|d| d.to_str())
            .collect::<Vec<_>>()
            .join(",");
        if !dirs_str.is_empty() {
            cmd.arg("--dirs").arg(&dirs_str);
        }
    }

    cmd.arg(file_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    tracing::debug!("running compiler: {:?}", cmd);

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to run compiler: {e}");
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    tracing::debug!("compiler output: {combined}");

    parse_compiler_output(&combined)
}

/// Collect all directories containing .nss files under the given roots.
pub fn collect_nss_directories(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in roots {
        collect_dirs_recursive(root, &mut dirs);
    }
    dirs
}

fn collect_dirs_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut has_nss = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip .nasher cache directories
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            collect_dirs_recursive(&path, out);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("nss")) {
            has_nss = true;
        }
    }

    if has_nss && !out.contains(&dir.to_path_buf()) {
        out.push(dir.to_path_buf());
    }
}

/// Parse nwn_script_comp output into diagnostics.
///
/// Output format:
///   `F [...] filepath: filename(LINE): ERROR: MESSAGE [Nms]`
///   `E [...] error message`
fn parse_compiler_output(output: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(diag) = parse_compiler_line(line) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

fn parse_compiler_line(line: &str) -> Option<Diagnostic> {
    // Format: `F/E [timestamp] path: file(LINE): ERROR: MESSAGE [Nms]`
    // Or:     `F/E [timestamp] path: file(LINE): ERROR: NSCxxxx: MESSAGE [Nms]`

    // Must start with F or E (fatal/error severity indicator)
    let severity = match line.chars().next()? {
        'F' => DiagnosticSeverity::ERROR,
        'E' => DiagnosticSeverity::ERROR,
        'W' => DiagnosticSeverity::WARNING,
        _ => return None,
    };

    // Look for the ERROR: or WARNING: marker
    let error_marker = if let Some(idx) = line.find("ERROR:") {
        Some((idx, DiagnosticSeverity::ERROR, 6))
    } else if let Some(idx) = line.find("WARNING:") {
        Some((idx, DiagnosticSeverity::WARNING, 8))
    } else {
        None
    };

    let (msg_area_start, sev, marker_len) = error_marker?;
    let severity = sev;

    // Extract the message after "ERROR: " or "WARNING: "
    let after_marker = &line[msg_area_start + marker_len..].trim();
    // Strip trailing [Nms] timing info
    let message = after_marker
        .rfind(" [")
        .map(|i| &after_marker[..i])
        .unwrap_or(after_marker)
        .to_string();

    // Try to extract line number from filename(LINE) pattern
    let line_num = extract_line_number(line).unwrap_or(1);

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
    // Look for filename(LINE) pattern
    // The pattern appears after the source file reference
    // e.g., "test_err.nss(1): ERROR: ..."
    let mut i = 0;
    let bytes = line.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b')' {
                if let Ok(n) = line[start..end].parse::<u32>() {
                    // Verify this is followed by ): (the error separator)
                    if end + 1 < bytes.len() && bytes[end + 1] == b':' {
                        return Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    None
}
