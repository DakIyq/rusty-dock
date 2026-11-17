//! Game launchers the dock can read installed games from.

mod battlenet;
mod ea;
mod steam;
mod vdf;

pub use battlenet::launch as launch_battlenet;

use std::path::{Path, PathBuf};

use crate::config::CustomLauncher;
use crate::game::{Game, Launch, Launcher};

/// A file's name without its extension ("Tool.lnk" -> "Tool"), as a custom item's title.
pub fn file_title(path: &Path) -> String {
    path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The launchers named in `order` ("steam", "battlenet", "ea") that are installed on this
/// machine, in that order, each with its games most recently played first and `hidden` game
/// ids left out, followed by the user's `custom` launchers (no games yet).
pub fn scan_all(order: &[String], custom: &[CustomLauncher], hidden: &[String]) -> Vec<Launcher> {
    let mut seen: Vec<String> = Vec::new();
    let mut launchers: Vec<Launcher> = Vec::new();
    for key in order {
        let key = key.trim().to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        let launcher = match key.as_str() {
            "steam" => steam::scan(),
            "battlenet" | "battle.net" => battlenet::scan(),
            "ea" => ea::scan(),
            _ => {
                eprintln!("unknown launcher {key:?} in settings");
                None
            }
        };
        launchers.extend(launcher);
        seen.push(key);
    }
    for custom in custom {
        let name = custom.name.trim();
        let key = custom.key();
        if name.is_empty() || seen.contains(&key.to_ascii_lowercase()) {
            continue;
        }
        seen.push(key.to_ascii_lowercase());
        let icon = custom.icon.trim();
        let games = custom
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.path.trim().is_empty())
            .map(|(i, item)| {
                let path = PathBuf::from(item.path.trim());
                let title = match item.name.trim() {
                    "" => file_title(&path),
                    name => name.to_string(),
                };
                // Shown as an icon (main.rs `icon_tile`): the chosen one, else the program's own.
                let cover = match item.icon.trim() {
                    "" => path.clone(),
                    icon => PathBuf::from(icon),
                };
                Game { id: format!("{key}:{i}"), title, cover: Some(cover), launch: Launch::Open(path), last_played: 0 }
            })
            .collect();
        launchers.push(Launcher {
            key,
            name: name.to_string(),
            icon: (!icon.is_empty()).then(|| PathBuf::from(icon)),
            games,
            landscape_covers: false,
        });
    }
    for launcher in &mut launchers {
        launcher.games.retain(|game| !hidden.iter().any(|id| id.trim().eq_ignore_ascii_case(&game.id)));
        launcher.games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
    }
    launchers
}
