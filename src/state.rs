//! State the app keeps between runs, in `state.toml` next to `config.toml`. Unlike the
//! settings it is rewritten by the app, so it holds no comments worth keeping.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "state.toml";

#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub dock: DockState,
}

/// Where the user last dragged the dock: its top-left corner in physical pixels. `None` (the
/// first run) centers it.
#[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DockState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Launcher keys in the order the user dragged them into; empty keeps the settings' order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
}

/// Reads the state file. Fails soft: a missing, unreadable or invalid file yields the defaults.
pub fn load() -> State {
    let Some(path) = path() else { return State::default() };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
            eprintln!("ignoring {}: {err}", path.display());
            State::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(err) => {
            eprintln!("failed to read {}: {err}", path.display());
            State::default()
        }
    }
}

/// Changes part of the saved state, keeping the rest as it is on disk.
pub fn update(change: impl FnOnce(&mut State)) {
    let mut state = load();
    change(&mut state);
    save(&state);
}

/// Writes the state file, logging failures.
pub fn save(state: &State) {
    let Some(path) = path() else { return };
    let result = toml::to_string(state).map_err(std::io::Error::other).and_then(|text| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, text)
    });
    if let Err(err) = result {
        eprintln!("failed to write {}: {err}", path.display());
    }
}

fn path() -> Option<PathBuf> {
    Some(crate::config::dir()?.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let order = vec!["ea".to_string(), "steam".to_string()];
        let state = State { dock: DockState { x: Some(-120), y: Some(980), order } };
        assert_eq!(toml::from_str::<State>(&toml::to_string(&state).unwrap()).unwrap(), state);
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(toml::from_str::<State>("").unwrap(), State::default());
    }
}
