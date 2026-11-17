//! Installed EA app games, read from the `EA Games` registry keys EA's installer writes.
//! Each game's `__Installer\installerdata.xml` holds the content id the EA app launches it by.
//! The EA app keeps no portrait cover art locally, so its games get landscape tiles showing
//! their 16:9 splash art.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::game::{Game, Launch, Launcher};
use crate::platform;

/// The EA app's own registry key; both views exist, the native one is checked first.
const CLIENT_KEYS: [&str; 2] = [
    r"SOFTWARE\Electronic Arts\EA Desktop",
    r"SOFTWARE\WOW6432Node\Electronic Arts\EA Desktop",
];
const DEFAULT_CLIENT_EXE: &str = r"C:\Program Files\Electronic Arts\EA Desktop\EA Desktop\EADesktop.exe";
/// One subkey per installed game, holding `Install Dir` and `DisplayName`.
const GAME_KEYS: [&str; 2] = [r"SOFTWARE\EA Games", r"SOFTWARE\WOW6432Node\EA Games"];

/// The EA app and its installed games, or `None` if the EA app isn't installed.
pub fn scan() -> Option<Launcher> {
    let exe = client_exe()?;
    let played = read_last_played();
    let splash = data_dir().map(|dir| dir.join("SplashArtwork"));

    let games = installed_games()
        .into_iter()
        .map(|(content_id, title)| Game {
            id: format!("ea:{content_id}"),
            title,
            cover: splash.as_deref().and_then(|dir| splash_art(dir, &content_id)),
            launch: Launch::Uri(format!("origin2://game/launch?offerIds={content_id}")),
            last_played: played.get(&content_id).copied().unwrap_or(0),
        })
        .collect();

    Some(Launcher { key: "ea".into(), name: "EA".into(), icon: Some(exe), games, landscape_covers: true })
}

fn client_exe() -> Option<PathBuf> {
    CLIENT_KEYS
        .iter()
        .filter_map(|key| platform::read_hklm_string(key, "ClientPath"))
        .map(PathBuf::from)
        .chain(Some(PathBuf::from(DEFAULT_CLIENT_EXE)))
        .find(|exe| exe.is_file())
}

/// (content id, title) of every installed game.
fn installed_games() -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for root in GAME_KEYS {
        for name in platform::hklm_subkeys(root) {
            let key = format!(r"{root}\{name}");
            let Some(dir) = platform::read_hklm_string(&key, "Install Dir") else { continue };
            let dir = PathBuf::from(dir);
            // EA games bought on Steam register here too, but Steam's dock already lists them.
            if !dir.is_dir() || dir.to_string_lossy().to_lowercase().contains("steamapps") {
                continue;
            }
            let Some(content_id) = read_content_id(&dir) else { continue };
            if seen.insert(content_id.clone()) {
                let title = platform::read_hklm_string(&key, "DisplayName").unwrap_or(name);
                games.push((content_id, title));
            }
        }
    }
    games
}

fn read_content_id(install_dir: &Path) -> Option<String> {
    let xml = fs::read(install_dir.join("__Installer").join("installerdata.xml")).ok()?;
    parse_content_id(&String::from_utf8_lossy(&xml))
}

/// The first `<contentID>` of an installerdata.xml. Ids end up in a launch URI, so only
/// characters EA ids are known to use are accepted.
fn parse_content_id(xml: &str) -> Option<String> {
    let start = xml.find("<contentID>")? + "<contentID>".len();
    let end = start + xml[start..].find("</contentID>")?;
    let id = xml[start..end].trim();
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    valid.then(|| id.to_string())
}

/// When each game was last played, keyed by content id. The EA app records no play times
/// locally; the closest is the cloud save sync after a session, which writes
/// `%LOCALAPPDATA%\Electronic Arts\EA Desktop\cloudsync\<n>_<contentID>[_beta].lastsync`.
/// Games without cloud saves get 0.
fn read_last_played() -> HashMap<String, u64> {
    let mut played = HashMap::new();
    let Some(dir) = data_dir().map(|dir| dir.join("cloudsync")) else { return played };
    let Ok(entries) = fs::read_dir(dir) else { return played };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(content_id) = sync_content_id(&name) else { continue };
        let Some(secs) = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|age| age.as_secs())
        else {
            continue;
        };
        let latest = played.entry(content_id.to_string()).or_insert(0);
        *latest = (*latest).max(secs);
    }
    played
}

/// `%LOCALAPPDATA%\Electronic Arts\EA Desktop`, the EA app's per-user data.
fn data_dir() -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    Some(local.join("Electronic Arts").join("EA Desktop"))
}

/// The game's splash art (1536x864), shown by the EA app while a game starts.
/// `<sha256(contentID)>.alias` holds the hash naming the image, `<hash>-hero.jpg`.
fn splash_art(dir: &Path, content_id: &str) -> Option<PathBuf> {
    let id_hash = sha256_hex(content_id);
    let alias = fs::read_to_string(dir.join(format!("{id_hash}.alias"))).ok();
    let hash = alias.as_deref().map(str::trim).unwrap_or(id_hash.as_str());
    // The alias ends up in a path, so anything but a hex hash could point outside the folder.
    if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(dir.join(format!("{hash}-hero.jpg"))).filter(|path| path.is_file())
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The content id in a sync file name such as `190132_1026023.lastsync`.
fn sync_content_id(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_suffix(".lastsync")?;
    stem.split('_').nth(1).filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_id_from_installer_data() {
        let xml = "<DiPManifest>\n  <contentIDs>\n    <contentID>1026023</contentID>\n    <contentID>999</contentID>\n  </contentIDs>";
        assert_eq!(parse_content_id(xml).as_deref(), Some("1026023"));
        assert_eq!(parse_content_id("<contentID>Origin.OFR.50.0001</contentID>").as_deref(), Some("Origin.OFR.50.0001"));
        assert_eq!(parse_content_id("<contentID>1&amp;x=2</contentID>"), None);
        assert_eq!(parse_content_id("<contentID></contentID>"), None);
        assert_eq!(parse_content_id("<contentIDs></contentIDs>"), None);
    }

    #[test]
    fn hashes_content_ids_like_the_splash_artwork_names() {
        // Battlefield 1's alias file in SplashArtwork.
        assert_eq!(sha256_hex("1026023"), "785fbdb21a5976f2262160d4c50a8b787fe7af26560003a3dffe6bd7964d3566");
    }

    #[test]
    fn reads_content_id_from_sync_file_names() {
        assert_eq!(sync_content_id("190132_1026023.lastsync"), Some("1026023"));
        assert_eq!(sync_content_id("196736_196736_beta.lastsync"), Some("196736"));
        assert_eq!(sync_content_id("190132_1026023.user"), None);
        assert_eq!(sync_content_id("190132.lastsync"), None);
    }
}
