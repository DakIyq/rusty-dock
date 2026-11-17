use std::path::PathBuf;

/// A game launcher (Steam, Battle.net, ..., or one the user added) and the games installed
/// through it.
pub struct Launcher {
    /// Its name in the settings' `launchers` list and the saved dock order, e.g. "battlenet";
    /// "custom:<name>" for a custom launcher.
    pub key: String,
    pub name: String,
    /// Its dock icon: an executable (its icon is used) or an image file.
    pub icon: Option<PathBuf>,
    pub games: Vec<Game>,
    /// Whether its covers are 16:9 landscape art rather than 2:3 portrait.
    pub landscape_covers: bool,
}

/// An installed game, launched through its own launcher so login, DRM and updates still work.
pub struct Game {
    /// Unique across launchers, e.g. "steam:570".
    pub id: String,
    pub title: String,
    /// Cover art shown in the games popup; its shape is the launcher's `landscape_covers`.
    pub cover: Option<PathBuf>,
    pub launch: Launch,
    /// Unix seconds; 0 if never played.
    pub last_played: u64,
}

/// How a launcher is asked to start a game.
pub enum Launch {
    /// URI handled by the launcher, e.g. "steam://rungameid/570".
    Uri(String),
    /// `Battle.net.exe --exec="launch <code>"`; without a code it just opens Battle.net.
    Battlenet { exe: PathBuf, code: Option<String> },
    /// A program, shortcut or other file from a custom launcher, opened like a double-click.
    Open(PathBuf),
}

impl Launcher {
    /// Whether the user added it (Manage Dock > Custom), rather than it being scanned.
    pub fn is_custom(&self) -> bool {
        self.key.starts_with(crate::config::CUSTOM_KEY_PREFIX)
    }
}

impl Launch {
    pub fn start(&self) {
        match self {
            Launch::Uri(uri) => crate::platform::open_uri(uri),
            Launch::Open(path) => crate::platform::open_file(path),
            Launch::Battlenet { exe, code } => crate::providers::launch_battlenet(exe, code.as_deref()),
        }
    }
}
