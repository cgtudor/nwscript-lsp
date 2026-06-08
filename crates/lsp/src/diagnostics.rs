use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// Runs `nwn_script_comp` on a file and parses the output into LSP diagnostics.
///
/// If a nasher cache directory is available, the file is written to a temp
/// location and compiled using the cache as `--dirs`. This avoids duplicate
/// function errors that occur when multiple source directories are passed.
pub async fn compile_file(
    compiler_path: &Path,
    file_path: &Path,
    source: &str,
    nasher_cache: &Option<PathBuf>,
    extra_dirs: &[PathBuf],
) -> Vec<Diagnostic> {
    let mut cmd = Command::new(compiler_path);

    // Simulate: don't write output files
    cmd.arg("-s");
    // No entry point required (works for include files too)
    cmd.arg("-n");
    // Collect all errors
    cmd.arg("-E");
    // Suppress info logging
    cmd.arg("--quiet");

    // Strategy: write current source into the nasher cache directory and compile
    // from there. The compiler resolves includes from the compiled file's
    // directory, so being IN the cache ensures --dirs + file-dir have the same
    // files, matching how nasher itself compiles. The original is backed up and
    // restored after compilation.
    let (compile_path, backup) = if let Some(cache_dir) = nasher_cache {
        let file_name = file_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("temp.nss"));
        let cache_file = cache_dir.join(file_name);

        // Backup the existing cache copy (if any)
        let backup_path = cache_dir.join(format!("{}.lsp-bak", file_name.to_string_lossy()));
        let had_original = cache_file.exists();
        if had_original {
            let _ = std::fs::copy(&cache_file, &backup_path);
        }

        // Write current (possibly modified) source into cache
        if std::fs::write(&cache_file, source).is_err() {
            if had_original {
                let _ = std::fs::rename(&backup_path, &cache_file);
            }
            return Vec::new();
        }

        cmd.arg("--dirs").arg(cache_dir);
        (cache_file, Some((backup_path, had_original)))
    } else {
        // No nasher cache: fall back to passing extra dirs
        if !extra_dirs.is_empty() {
            let dirs_str = extra_dirs
                .iter()
                .filter_map(|d| d.to_str())
                .collect::<Vec<_>>()
                .join(",");
            if !dirs_str.is_empty() {
                cmd.arg("--dirs").arg(&dirs_str);
            }
        }
        (file_path.to_path_buf(), None)
    };

    cmd.arg(&compile_path);
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

    // Restore the original cache file
    if let Some((backup_path, had_original)) = backup {
        if had_original {
            let _ = std::fs::rename(&backup_path, &compile_path);
        } else {
            // File didn't exist before — remove our copy and the backup
            let _ = std::fs::remove_file(&compile_path);
            let _ = std::fs::remove_file(&backup_path);
        }
    }

    parse_compiler_output(&combined)
}

/// Find the nasher cache directory for a workspace.
pub fn find_nasher_cache(workspace_dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in workspace_dirs {
        // Check direct nasher cache
        let cache = dir.join(".nasher").join("cache");
        if cache.is_dir() {
            // Use the first target's cache
            if let Ok(entries) = std::fs::read_dir(&cache) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Verify it contains .nss files
                        let has_nss = std::fs::read_dir(&path)
                            .map(|entries| {
                                entries.flatten().any(|e| {
                                    e.path()
                                        .extension()
                                        .is_some_and(|ext| ext.eq_ignore_ascii_case("nss"))
                                })
                            })
                            .unwrap_or(false);
                        if has_nss {
                            return Some(path);
                        }
                    }
                }
            }
        }

        // Also check subdirectories (multi-repo workspace)
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let sub = entry.path();
                if sub.is_dir() {
                    let cache = sub.join(".nasher").join("cache");
                    if cache.is_dir() {
                        if let Ok(entries) = std::fs::read_dir(&cache) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() {
                                    let has_nss = std::fs::read_dir(&path)
                                        .map(|entries| {
                                            entries.flatten().any(|e| {
                                                e.path().extension().is_some_and(|ext| {
                                                    ext.eq_ignore_ascii_case("nss")
                                                })
                                            })
                                        })
                                        .unwrap_or(false);
                                    if has_nss {
                                        return Some(path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
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
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.')
                || name == "docs"
                || name == "nwn_source"
                || name == "node_modules"
                || name == "target"
            {
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
    // Must start with F or E (fatal/error) or W (warning)
    let severity = match line.chars().next()? {
        'F' | 'E' => DiagnosticSeverity::ERROR,
        'W' => DiagnosticSeverity::WARNING,
        _ => return None,
    };

    // Look for ERROR: or WARNING: marker
    let (msg_start, severity, marker_len) = if let Some(idx) = line.find("ERROR:") {
        (idx, DiagnosticSeverity::ERROR, 6)
    } else if let Some(idx) = line.find("WARNING:") {
        (idx, DiagnosticSeverity::WARNING, 8)
    } else {
        return None;
    };

    // Extract message after marker
    let after_marker = line[msg_start + marker_len..].trim();
    // Strip trailing [Nms] timing info
    let raw_message = after_marker
        .rfind(" [")
        .map(|i| &after_marker[..i])
        .unwrap_or(after_marker)
        .trim();

    if raw_message.is_empty() {
        return None;
    }

    // Extract line number and source file
    let (line_num, source_file) = extract_location_info(line);
    let line_num = line_num.unwrap_or(1);

    // Build message with context
    let message = match source_file {
        Some(src) => format!("{raw_message} (in {src}:{line_num})"),
        None => raw_message.to_string(),
    };

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

/// Extract (line_number, source_filename) from compiler output.
fn extract_location_info(line: &str) -> (Option<u32>, Option<String>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b')' {
                if let Ok(n) = line[start..end].parse::<u32>() {
                    if end + 1 < bytes.len() && bytes[end + 1] == b':' {
                        let before = &line[..i];
                        let filename = before
                            .rsplit(|c: char| c == ' ' || c == ':' || c == '/' || c == '\\')
                            .next()
                            .filter(|s| s.ends_with(".nss"))
                            .map(|s| s.to_string());
                        return (Some(n), filename);
                    }
                }
            }
        }
        i += 1;
    }
    (None, None)
}
