//! Installed Steam games, read from Steam's local library files.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::vdf::{self, Value};
use crate::game::{Game, Launch, Launcher};

/// Steamworks Common Redistributables: installed alongside many games, not launchable.
const REDISTRIBUTABLES_APP_ID: &str = "228980";
/// `StateFlags` bit that is set once an app is fully installed.
const STATE_FULLY_INSTALLED: u64 = 4;

/// Steam and its installed games, or `None` if Steam isn't installed.
pub fn scan() -> Option<Launcher> {
    let root = steam_root()?;
    Some(Launcher {
        key: "steam".into(),
        name: "Steam".into(),
        icon: Some(root.join("steam.exe")).filter(|exe| exe.is_file()),
        games: installed_games(&root),
        landscape_covers: false,
    })
}

fn installed_games(root: &Path) -> Vec<Game> {
    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for library in library_folders(root) {
        let Ok(entries) = fs::read_dir(library.join("steamapps")) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_manifest = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"));
            if !is_manifest {
                continue;
            }
            let Some(game) = read_manifest(root, &path) else { continue };
            // Steam's own folder usually also appears in libraryfolders.vdf, so skip repeats.
            if seen.insert(game.id.clone()) {
                games.push(game);
            }
        }
    }
    games
}

fn read_manifest(root: &Path, path: &Path) -> Option<Game> {
    let doc = vdf::parse(&String::from_utf8_lossy(&fs::read(path).ok()?));
    let app = doc.get("AppState")?;
    let app_id = app.get_str("appid")?;
    let title = app.get_str("name")?;
    let flags: u64 = app.get_str("StateFlags")?.parse().ok()?;
    if flags & STATE_FULLY_INSTALLED == 0 || !is_game(app_id, title) {
        return None;
    }

    Some(Game {
        id: format!("steam:{app_id}"),
        title: title.to_string(),
        cover: find_cover(root, app_id),
        launch: Launch::Uri(format!("steam://rungameid/{app_id}")),
        last_played: app.get_str("LastPlayed").and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

/// Filters out the runtime/tool apps Steam installs next to games.
fn is_game(app_id: &str, title: &str) -> bool {
    app_id != REDISTRIBUTABLES_APP_ID
        && !title.starts_with("Proton ")
        && !title.starts_with("Steam Linux Runtime")
}

/// Every Steam library folder: Steam's own folder plus any added in Steam's settings.
fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    let Ok(text) = fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) else {
        return folders;
    };
    let doc = vdf::parse(&text);
    let Some(list) = doc.get("libraryfolders") else { return folders };

    // Folders are keyed "0", "1", ...; other keys hold Steam's bookkeeping.
    for (key, entry) in list.entries() {
        if key.parse::<u32>().is_err() {
            continue;
        }
        // Current format: "0" { "path" "D:\\SteamLibrary" ... }; older format: "1" "D:\\SteamLibrary".
        let path = match entry {
            Value::Obj(_) => entry.get_str("path"),
            Value::Str(path) => Some(path.as_str()),
        };
        if let Some(path) = path {
            folders.push(PathBuf::from(path));
        }
    }
    folders
}

/// Cover art Steam has cached for the app: the portrait cover, otherwise the header banner.
fn find_cover(root: &Path, app_id: &str) -> Option<PathBuf> {
    let cache = root.join("appcache").join("librarycache");
    // Newer Steam keeps a folder per app; older Steam keeps flat files prefixed with the app id.
    let app_dir = cache.join(app_id);
    [
        app_dir.join("library_600x900.jpg"),
        cache.join(format!("{app_id}_library_600x900.jpg")),
        app_dir.join("header.jpg"),
        cache.join(format!("{app_id}_header.jpg")),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn steam_root() -> Option<PathBuf> {
    candidate_roots().into_iter().find(|path| path.join("steamapps").is_dir())
}

#[cfg(windows)]
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = crate::platform::read_hkcu_string(r"Software\Valve\Steam", "SteamPath") {
        roots.push(PathBuf::from(path));
    }
    roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    roots
}

#[cfg(not(windows))]
fn candidate_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support/Steam")]
    } else {
        vec![
            home.join(".steam/steam"),
            home.join(".local/share/Steam"),
            // Flatpak install.
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ]
    }
}
