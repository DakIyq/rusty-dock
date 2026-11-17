//! Installed Battle.net games, read from the Uninstall registry entries Blizzard's installer
//! writes. (The agent's `product.db` that older tools read no longer exists on current
//! Battle.net versions.) Cover art and launch codes come from the client's asset cache.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::game::{Game, Launch, Launcher};
use crate::platform;

/// Where Windows lists installed programs. Blizzard's installer is 32-bit, so its entries
/// usually land in the WOW6432Node view.
const UNINSTALL_KEYS: [&str; 2] = [
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
];
/// The uid of Battle.net's own Uninstall entry.
const CLIENT_UID: &str = "battle.net";
const CLIENT_EXE: &str = "Battle.net.exe";
/// How the client's per-game asset manifests start; the rest of its cache is images.
const MANIFEST_PREFIX: &[u8] = br#"{"files":"#;
/// What the client logs once it is logged in and can take commands.
const LOGIN_MARKER: &str = "Logged into Battle.net successfully";
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// A newly started client creates its log within a second or two.
const NEW_LOG_TIMEOUT: Duration = Duration::from_secs(10);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
const AFTER_LOGIN_DELAY: Duration = Duration::from_secs(3);

/// One Blizzard product found in the Uninstall registry.
struct Product {
    /// Battle.net's install uid, e.g. "s2"; also the key in `Battle.net.config`.
    uid: String,
    title: String,
    location: PathBuf,
}

/// Battle.net and its installed games, or `None` if Battle.net isn't installed.
pub fn scan() -> Option<Launcher> {
    let products = installed_products();
    let exe = client_exe(&products)?;
    let config = read_config();
    let cache = read_cache();

    let games = products
        .iter()
        .filter(|p| p.uid != CLIENT_UID && p.location.is_dir())
        .map(|p| {
            let cached = cache.get(&p.uid);
            let code = cached.and_then(|c| c.launch_code.as_deref()).or_else(|| launch_code(&p.uid));
            Game {
                id: format!("battlenet:{}", p.uid),
                title: p.title.clone(),
                cover: cached.and_then(|c| c.cover.clone()),
                launch: Launch::Battlenet { exe: exe.clone(), code: code.map(str::to_string) },
                last_played: last_played(config.as_ref(), &p.uid),
            }
        })
        .collect();

    Some(Launcher { key: "battlenet".into(), name: "Battle.net".into(), icon: Some(exe), games, landscape_covers: false })
}

fn installed_products() -> Vec<Product> {
    let mut seen = HashSet::new();
    let mut products = Vec::new();
    for root in UNINSTALL_KEYS {
        for name in platform::hklm_subkeys(root) {
            let key = format!(r"{root}\{name}");
            let Some(uninstall) = platform::read_hklm_string(&key, "UninstallString") else { continue };
            // Only Blizzard's uninstaller takes `--uid=`, so this also filters out other programs.
            if !uninstall.contains("Blizzard Uninstaller") {
                continue;
            }
            let Some(uid) = parse_uid(&uninstall) else { continue };
            let Some(location) = platform::read_hklm_string(&key, "InstallLocation") else { continue };
            let title = platform::read_hklm_string(&key, "DisplayName").unwrap_or_else(|| name.clone());
            if seen.insert(uid.clone()) {
                products.push(Product { uid, title, location: PathBuf::from(location) });
            }
        }
    }
    products
}

/// The uid from an uninstall command such as
/// `"...\Blizzard Uninstaller.exe" --lang=enUS --uid=s2 --displayname="StarCraft II"`.
fn parse_uid(uninstall: &str) -> Option<String> {
    let start = uninstall.find("--uid=")? + "--uid=".len();
    let uid: String = uninstall[start..]
        .trim_start_matches('"')
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '"')
        .collect();
    Some(uid).filter(|uid| !uid.is_empty())
}

fn client_exe(products: &[Product]) -> Option<PathBuf> {
    let registered = products.iter().find(|p| p.uid == CLIENT_UID).map(|p| p.location.join(CLIENT_EXE));
    registered
        .into_iter()
        .chain(Some(PathBuf::from(r"C:\Program Files (x86)\Battle.net").join(CLIENT_EXE)))
        .find(|exe| exe.is_file())
}

/// Starts a product through Battle.net. A client started by `--exec` reads the command but
/// drops it while logging in (its log shows the argument, then the login, then nothing), so
/// when this starts a new client the command is sent again once that client has logged in;
/// a running client gets it forwarded and launches right away.
pub fn launch(exe: &Path, code: Option<&str>) {
    let started = SystemTime::now();
    if !spawn_client(exe, code) {
        return;
    }
    let Some(code) = code.map(str::to_string) else { return };
    let exe = exe.to_path_buf();
    std::thread::spawn(move || {
        if wait_for_login(started) {
            // The client needs a moment after login before it accepts commands.
            std::thread::sleep(AFTER_LOGIN_DELAY);
            spawn_client(&exe, Some(&code));
        }
    });
}

fn spawn_client(exe: &Path, code: Option<&str>) -> bool {
    let mut command = std::process::Command::new(exe);
    command.args(launch_args(code));
    // The client may look for its files relative to the working directory.
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    match command.spawn() {
        Ok(_) => true,
        Err(err) => {
            eprintln!("failed to start {}: {err}", exe.display());
            false
        }
    }
}

/// Whether a client started at or after `started` logged in. Each client writes a new
/// `%LOCALAPPDATA%\Battle.net\Logs\battle.net-<time>.log`; none appearing means the command
/// went to a client that was already running.
fn wait_for_login(started: SystemTime) -> bool {
    let Some(logs) = std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Battle.net").join("Logs"))
    else {
        return false;
    };
    let mut log: Option<PathBuf> = None;
    loop {
        std::thread::sleep(LOG_POLL_INTERVAL);
        let waited = started.elapsed().unwrap_or_default();
        if log.is_none() {
            log = new_client_log(&logs, started);
            if log.is_none() && waited > NEW_LOG_TIMEOUT {
                return false;
            }
        }
        if let Some(path) = &log {
            let text = fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
            if text.contains(LOGIN_MARKER) {
                return true;
            }
        }
        // Login can wait on a security checkpoint or the user typing a password.
        if waited > LOGIN_TIMEOUT {
            return false;
        }
    }
}

/// The log of a client started at or after `started`, if one has appeared.
fn new_client_log(logs: &Path, started: SystemTime) -> Option<PathBuf> {
    fs::read_dir(logs).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let created = entry.metadata().and_then(|meta| meta.created()).ok()?;
        (is_client_log(&name) && created >= started).then(|| entry.path())
    })
}

/// The client's own logs, not the `libcef-*` or agent logs in the same folder.
fn is_client_log(name: &str) -> bool {
    name.starts_with("battle.net-") && name.ends_with(".log")
}

/// Arguments that make Battle.net launch the product, or none (just open Battle.net) when
/// its launch code isn't known.
fn launch_args(code: Option<&str>) -> Vec<String> {
    match code {
        Some(code) => vec![format!("--exec=launch {code}")],
        None => Vec::new(),
    }
}

/// Battle.net's `--exec="launch <code>"` takes a product code that differs from the install
/// uid. Fallback for products the asset cache doesn't describe.
fn launch_code(uid: &str) -> Option<&'static str> {
    Some(match uid {
        "s1" => "S1",
        "s2" => "S2",
        "w3" => "W3",
        "wow" => "WoW",
        "wow_classic" => "WoWC",
        "diablo3" => "D3",
        "osi" => "OSI",
        "fenris" => "Fen",
        "anbs" => "ANBS",
        "prometheus" | "pro" => "Pro",
        "hs_beta" => "WTCG",
        "heroes" => "Hero",
        "rtro" => "RTRO",
        "wlby" => "WLBY",
        "odin" => "ODIN",
        "lazarus" => "LAZR",
        "viper" => "VIPR",
        "zeus" => "ZEUS",
        "fore" => "FORE",
        "auks" => "AUKS",
        _ => return None,
    })
}

/// What the asset cache knows about one install uid.
#[derive(Default)]
struct CachedProduct {
    /// The product id, which is also the launch code, e.g. "S2" for uid "s2".
    launch_code: Option<String>,
    /// Portrait (3:4) key art, as shown in the client's game library.
    cover: Option<PathBuf>,
}

/// Reads `%LOCALAPPDATA%\Battle.net\Cache`, where the client stores downloaded assets under
/// their hash as `<h[0..2]>\<h[2..4]>\<hash>`. Among the images are JSON manifests, one per
/// game: `files.default` maps asset keys (`starcraft_ii#S2_KEY_ART`) to hashes, and each of
/// `products` names its id, its `base.key_art` key and, under `base.types`, its install uids.
fn read_cache() -> HashMap<String, CachedProduct> {
    let mut products = HashMap::new();
    let Some(cache) = std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Battle.net").join("Cache"))
    else {
        return products;
    };
    let manifests = read_manifests(&cache);

    // Several copies of a game's manifest pile up; they are oldest first, so newer ones win.
    let mut hashes: HashMap<&str, &str> = HashMap::new();
    for manifest in &manifests {
        let Some(files) = manifest["files"]["default"].as_object() else { continue };
        for (key, file) in files {
            if let Some(hash) = file["hash"].as_str() {
                hashes.insert(key.as_str(), hash);
            }
        }
    }

    for manifest in &manifests {
        let Some(list) = manifest["products"].as_array() else { continue };
        for product in list {
            let base = &product["base"];
            let cover = base["key_art"]
                .as_str()
                .and_then(|key| hashes.get(key))
                .and_then(|hash| asset_path(&cache, hash));
            let Some(types) = base["types"].as_object() else { continue };
            for uid in types.values().filter_map(|t| t["uid"].as_str()) {
                let entry: &mut CachedProduct = products.entry(uid.to_string()).or_default();
                if let Some(code) = product["id"].as_str() {
                    entry.launch_code = Some(code.to_string());
                }
                // Older manifests lack key art; don't let them erase a newer one's.
                if cover.is_some() {
                    entry.cover = cover.clone();
                }
            }
        }
    }
    products
}

/// Every manifest in the cache, oldest first.
fn read_manifests(cache: &Path) -> Vec<Value> {
    let mut files = Vec::new();
    collect_files(cache, 2, &mut files);
    let mut found: Vec<(SystemTime, PathBuf)> = files
        .into_iter()
        .filter(|(_, path)| is_manifest(path))
        .collect();
    found.sort();
    found
        .iter()
        .filter_map(|(_, path)| serde_json::from_slice(&fs::read(path).ok()?).ok())
        .collect()
}

/// Files up to `depth` folders below `dir`, with their modification times.
fn collect_files(dir: &Path, depth: u32, out: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() && depth > 0 {
            collect_files(&entry.path(), depth - 1, out);
        } else if meta.is_file() {
            out.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), entry.path()));
        }
    }
}

/// Checks only the first bytes, so the cached images aren't read in full.
fn is_manifest(path: &Path) -> bool {
    let mut head = [0u8; MANIFEST_PREFIX.len()];
    let read = fs::File::open(path).and_then(|mut file| file.read_exact(&mut head));
    read.is_ok() && head[..] == *MANIFEST_PREFIX
}

/// The cached file for an asset hash, if the client has downloaded it.
fn asset_path(cache: &Path, hash: &str) -> Option<PathBuf> {
    // Hashes are hex; anything else would break the slicing or point outside the cache.
    if hash.len() < 4 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(cache.join(&hash[..2]).join(&hash[2..4]).join(hash)).filter(|path| path.is_file())
}

/// Battle.net's per-user settings, which record when each game was last played.
fn read_config() -> Option<serde_json::Value> {
    let appdata = PathBuf::from(std::env::var_os("APPDATA")?);
    let text = fs::read_to_string(appdata.join("Battle.net").join("Battle.net.config")).ok()?;
    serde_json::from_str(&text).ok()
}

/// `Games.<uid>.LastPlayed` from the config: Unix seconds stored as a string.
fn last_played(config: Option<&serde_json::Value>, uid: &str) -> u64 {
    config
        .and_then(|c| c["Games"][uid]["LastPlayed"].as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uid_from_uninstall_string() {
        let cmd = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=enUS --uid=s2 --displayname="StarCraft II""#;
        assert_eq!(parse_uid(cmd).as_deref(), Some("s2"));
        assert_eq!(parse_uid(r#"x.exe --uid="wow_classic""#).as_deref(), Some("wow_classic"));
        assert_eq!(parse_uid("x.exe --uid= --lang=enUS"), None);
        assert_eq!(parse_uid("x.exe"), None);
    }

    #[test]
    fn reads_last_played_from_config() {
        let config: serde_json::Value =
            serde_json::from_str(r#"{"Games":{"s2":{"LastPlayed":"1793521241"},"odin":{}}}"#).unwrap();
        assert_eq!(last_played(Some(&config), "s2"), 1793521241);
        assert_eq!(last_played(Some(&config), "odin"), 0);
        assert_eq!(last_played(Some(&config), "wow"), 0);
        assert_eq!(last_played(None, "s2"), 0);
    }

    #[test]
    fn unknown_products_just_open_battlenet() {
        assert_eq!(launch_args(launch_code("s2")), vec!["--exec=launch S2".to_string()]);
        assert!(launch_args(launch_code("something_new")).is_empty());
    }

    #[test]
    fn recognizes_client_logs() {
        assert!(is_client_log("battle.net-20261101t110213.500933.log"));
        assert!(!is_client_log("libcef-20261101t110216.452209.log"));
        assert!(!is_client_log("battle.net-20261101t110213.500933.log.old"));
    }

    #[test]
    fn rejects_asset_hashes_that_are_not_hex() {
        let cache = Path::new("cache");
        assert_eq!(asset_path(cache, r"..\..\x"), None);
        assert_eq!(asset_path(cache, "ab"), None);
    }
}
