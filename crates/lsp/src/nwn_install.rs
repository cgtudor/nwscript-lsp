//! NWN:EE installation discovery.
//!
//! Searches for the game installation in the following order:
//! 1. Explicit path from config (`nwnRoot` setting)
//! 2. `NWN_ROOT` environment variable
//! 3. Steam installation (platform-specific common paths)
//! 4. Beamdog Client installation (reads settings.json)
//! 5. GOG installation (platform-specific common paths)

use std::path::{Path, PathBuf};

/// Find the NWN:EE installation directory.
///
/// Returns `None` if no valid installation is found.
/// A valid installation contains a `data/` subdirectory.
pub fn find_nwn_root(explicit_path: Option<&str>) -> Option<PathBuf> {
    // 1. Explicit path from config
    if let Some(path) = explicit_path {
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if is_valid_nwn_root(&p) {
                return Some(p);
            }
            tracing::warn!(
                "configured nwnRoot is not a valid NWN installation: {}",
                p.display()
            );
        }
    }

    // 2. NWN_ROOT environment variable
    if let Ok(root) = std::env::var("NWN_ROOT") {
        let p = PathBuf::from(&root);
        if is_valid_nwn_root(&p) {
            tracing::info!("found NWN root via NWN_ROOT env var: {}", p.display());
            return Some(p);
        }
    }

    // 3. Steam
    if let Some(p) = find_steam_install() {
        tracing::info!("found NWN root via Steam: {}", p.display());
        return Some(p);
    }

    // 4. Beamdog Client
    if let Some(p) = find_beamdog_install() {
        tracing::info!("found NWN root via Beamdog Client: {}", p.display());
        return Some(p);
    }

    // 5. GOG
    if let Some(p) = find_gog_install() {
        tracing::info!("found NWN root via GOG: {}", p.display());
        return Some(p);
    }

    None
}

/// Check if a path looks like a valid NWN:EE installation.
fn is_valid_nwn_root(path: &Path) -> bool {
    path.is_dir() && path.join("data").is_dir()
}

// =============================================================================
// Steam
// =============================================================================

fn find_steam_install() -> Option<PathBuf> {
    let candidates = steam_candidates();
    for candidate in candidates {
        if is_valid_nwn_root(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn steam_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Default Steam path
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        candidates.push(
            PathBuf::from(pf86)
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Neverwinter Nights"),
        );
    }

    // Also check Program Files (non-x86)
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        candidates.push(
            PathBuf::from(pf)
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Neverwinter Nights"),
        );
    }

    candidates
}

#[cfg(target_os = "macos")]
fn steam_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Neverwinter Nights"),
        );
    }
    candidates
}

#[cfg(target_os = "linux")]
fn steam_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join(".local")
                .join("share")
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Neverwinter Nights"),
        );
    }
    candidates
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn steam_candidates() -> Vec<PathBuf> {
    Vec::new()
}

// =============================================================================
// Beamdog Client
// =============================================================================

fn find_beamdog_install() -> Option<PathBuf> {
    let settings_path = beamdog_settings_path()?;
    if !settings_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&settings_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let folders = json.get("folders")?.as_array()?;

    // Check known torrent IDs for NWN:EE (stable and development)
    let torrent_ids = ["00785", "00829"];

    for folder in folders {
        let folder_str = folder.as_str()?;
        for tid in &torrent_ids {
            let candidate = PathBuf::from(folder_str).join(tid);
            if is_valid_nwn_root(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn beamdog_settings_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|appdata| PathBuf::from(appdata).join("Beamdog Client").join("settings.json"))
}

#[cfg(target_os = "macos")]
fn beamdog_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("Beamdog Client")
            .join("settings.json")
    })
}

#[cfg(target_os = "linux")]
fn beamdog_settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|config| config.join("Beamdog Client").join("settings.json"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn beamdog_settings_path() -> Option<PathBuf> {
    None
}

// =============================================================================
// GOG
// =============================================================================

fn find_gog_install() -> Option<PathBuf> {
    let candidates = gog_candidates();
    for candidate in candidates {
        if is_valid_nwn_root(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn gog_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        candidates.push(
            PathBuf::from(pf86)
                .join("GOG Galaxy")
                .join("Games")
                .join("Neverwinter Nights Enhanced Edition"),
        );
    }
    candidates
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn gog_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join("GOG Games")
                .join("Neverwinter Nights Enhanced Edition"),
        );
    }
    candidates
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn gog_candidates() -> Vec<PathBuf> {
    Vec::new()
}
