use std::path::{Path, PathBuf};

/// Parsed nasher.cfg configuration relevant to the language server.
#[derive(Debug, Default)]
pub struct NasherConfig {
    /// Source include glob patterns (e.g., "src/**/*.nss")
    pub source_patterns: Vec<String>,
    /// Directories containing source files (derived from patterns)
    pub source_dirs: Vec<PathBuf>,
}

/// Parse a nasher.cfg file and extract source directories.
pub fn parse_nasher_cfg(path: &Path) -> Option<NasherConfig> {
    let content = std::fs::read_to_string(path).ok()?;
    let base_dir = path.parent()?;

    let mut config = NasherConfig::default();
    let mut in_sources = false;

    for line in content.lines() {
        let line = line.trim();

        // Section headers
        if line.starts_with('[') {
            in_sources = line.eq_ignore_ascii_case("[sources]");
            continue;
        }

        if !in_sources {
            continue;
        }

        // Parse include = "pattern"
        if let Some(rest) = line.strip_prefix("include") {
            let rest = rest.trim().strip_prefix('=')?;
            let rest = rest.trim();
            let pattern = rest.trim_matches('"');

            config.source_patterns.push(pattern.to_string());

            // Extract the root directory from the glob pattern.
            // e.g., "src/**/*.nss" -> "src"
            if let Some(dir) = extract_base_dir(pattern) {
                let full_dir = base_dir.join(dir);
                if full_dir.is_dir() && !config.source_dirs.contains(&full_dir) {
                    config.source_dirs.push(full_dir);
                }
            }
        }
    }

    Some(config)
}

/// Extract the non-glob base directory from a glob pattern.
/// "src/**/*.nss" -> "src"
/// "module/**/*.json" -> "module"
fn extract_base_dir(pattern: &str) -> Option<&str> {
    // Find the first path component that doesn't contain glob chars
    let first_glob = pattern.find(|c| c == '*' || c == '?' || c == '{' || c == '[')?;
    let base = &pattern[..first_glob];
    let base = base.trim_end_matches('/').trim_end_matches('\\');
    if base.is_empty() {
        None
    } else {
        Some(base)
    }
}

/// Search for nasher.cfg files in a workspace directory and its parents.
pub fn find_nasher_configs(workspace_dir: &Path) -> Vec<PathBuf> {
    let mut configs = Vec::new();

    // Check current directory and subdirectories
    let candidates = [
        workspace_dir.join("nasher.cfg"),
    ];

    for path in &candidates {
        if path.exists() {
            configs.push(path.clone());
        }
    }

    // Also check immediate subdirectories (common in multi-repo workspaces)
    if let Ok(entries) = std::fs::read_dir(workspace_dir) {
        for entry in entries.flatten() {
            let sub_cfg = entry.path().join("nasher.cfg");
            if sub_cfg.exists() && !configs.contains(&sub_cfg) {
                configs.push(sub_cfg);
            }
        }
    }

    configs
}

/// Collect all source directories from all nasher.cfg files found in a workspace.
pub fn discover_source_dirs(workspace_dir: &Path) -> Vec<PathBuf> {
    let configs = find_nasher_configs(workspace_dir);
    let mut dirs = Vec::new();

    for cfg_path in configs {
        if let Some(config) = parse_nasher_cfg(&cfg_path) {
            for dir in config.source_dirs {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
    }

    // If no nasher configs found, use the workspace root itself
    if dirs.is_empty() {
        dirs.push(workspace_dir.to_path_buf());
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_base_dir_patterns() {
        assert_eq!(extract_base_dir("src/**/*.nss"), Some("src"));
        assert_eq!(extract_base_dir("module/**/*.{nss,json}"), Some("module"));
        assert_eq!(extract_base_dir("**/*.nss"), None);
        assert_eq!(extract_base_dir("*.nss"), None);
    }
}
