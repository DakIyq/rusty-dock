//! User settings, read from `config.toml` in the OS's config directory
//! (`%APPDATA%\RustyDock`, `~/.config/RustyDock`, `~/Library/Application Support/RustyDock`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

const DIR_NAME: &str = "RustyDock";
/// The folder's earlier name.
const OLD_DIR_NAME: &str = "rusty_dock";
const FILE_NAME: &str = "config.toml";
/// The file's earlier name, renamed on first run so the user's edits carry over.
const OLD_FILE_NAME: &str = "setting.toml";

/// The commented settings file holding `config`'s values. Written on first run so the user has
/// a file to edit, and by the settings window, which keeps the comments by regenerating them.
pub fn render(config: &Config) -> String {
    let (dock, popup) = (&config.dock, &config.popup);
    format!(
        r##"# Rusty Dock settings. Delete this file to restore the defaults.

# Launchers shown in the dock, in this order: "steam", "battlenet", "ea".
# Remove one to hide it. Once you drag the icons into another order, the dock keeps that one.
launchers = {launchers}

# Games left out of the popups, by id: "steam:<appid>", "battlenet:<uid>", "ea:<contentID>",
# e.g. hidden_games = ["steam:570"]
hidden_games = {hidden_games}

# Start Rusty Dock when you sign in (Windows). Applied each time the dock starts.
autostart = {autostart}

[dock]
# Size of the launcher icons, in pixels (before display scaling).
icon_size = {icon_size}
# Tile behind each launcher icon: "circle", "curve" (a rounded square) or "flat" (none, the icon
# as it is).
icon_shape = {icon_shape}
# Stack the icons in a column instead of a row. The popups then open beside the dock.
vertical = {vertical}
# Background color ("#RRGGBB") and opacity (0.0 transparent to 1.0 opaque).
background = {dock_background}
opacity = {dock_opacity}
# Keep the dock above other windows, maximised ones included (Windows). When false,
# maximised windows cover it until they are minimised, or until the Win key opens the Start
# menu: the dock then rises with it.
always_on_top = {always_on_top}
# Let the dock be dragged. When false it stays where it was last left.
movable = {movable}
# Let the launcher icons be dragged into another order. When false they keep the last one.
sortable = {sortable}

[popup]
# Game tiles per row, for portrait (Steam, Battle.net) and landscape (EA) covers.
columns = {columns}
landscape_columns = {landscape_columns}
# Rows shown before the popup scrolls.
rows = {rows}
# Tile behind each application icon in a custom launcher's popup: "circle", "curve" or "flat"
# (see [dock] icon_shape). Cover art always keeps its rounded corners.
icon_shape = {popup_icon_shape}
# Bubble color ("#RRGGBB") and opacity (0.0 transparent to 1.0 opaque).
background = {popup_background}
opacity = {popup_opacity}
# Close the popup when another app takes the foreground (Windows).
close_on_focus_loss = {close_on_focus_loss}

# Custom launchers, added from the app menu (Manage Dock > Custom), shown after the others
# until dragged elsewhere. The icon is an image or a program (its icon is used); leave it empty
# for the name's initials. Applications in its popup are added by right-clicking the popup:
# any program, shortcut or file, opened as if double-clicked, with an optional icon (empty:
# the program's own). Example:
# [[custom]]
# name = "My Launcher"
# icon = 'C:\Games\MyLauncher\launcher.exe'
#
# [[custom.items]]
# name = "My Game"
# path = 'C:\Games\MyGame\game.exe'
# icon = ""
{custom}"##,
        launchers = toml_array(&config.launchers),
        hidden_games = toml_array(&config.hidden_games),
        autostart = config.autostart,
        icon_size = dock.icon_size,
        icon_shape = toml_string(dock.icon_shape.name()),
        vertical = dock.vertical,
        dock_background = toml_string(&dock.background),
        dock_opacity = toml_float(dock.opacity),
        always_on_top = dock.always_on_top,
        movable = dock.movable,
        sortable = dock.sortable,
        columns = popup.columns,
        landscape_columns = popup.landscape_columns,
        rows = popup.rows,
        popup_icon_shape = toml_string(popup.icon_shape.name()),
        popup_background = toml_string(&popup.background),
        popup_opacity = toml_float(popup.opacity),
        close_on_focus_loss = popup.close_on_focus_loss,
        custom = render_custom(&config.custom),
    )
}

/// One `[[custom]]` table per custom launcher, each followed by its `[[custom.items]]`.
fn render_custom(custom: &[CustomLauncher]) -> String {
    let mut out = String::new();
    for launcher in custom {
        out += &format!("\n[[custom]]\nname = {}\nicon = {}\n", toml_string(&launcher.name), toml_string(&launcher.icon));
        for item in &launcher.items {
            out += &format!(
                "\n[[custom.items]]\nname = {}\npath = {}\nicon = {}\n",
                toml_string(&item.name),
                toml_string(&item.path),
                toml_string(&item.icon)
            );
        }
    }
    out
}

/// A quoted and escaped TOML string.
fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn toml_array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|s| toml_string(s)).collect();
    format!("[{}]", items.join(", "))
}

/// Rounded to two decimals, so a slider's 0.8999999 is written as 0.9; always with a decimal
/// point (`{:?}`), so it stays a TOML float.
fn toml_float(x: f64) -> String {
    let x = if x.is_finite() { (x * 100.0).round() / 100.0 } else { 1.0 };
    format!("{x:?}")
}

/// Missing keys take their default, so a partial file is fine.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub launchers: Vec<String>,
    pub hidden_games: Vec<String>,
    pub autostart: bool,
    pub dock: DockConfig,
    pub popup: PopupConfig,
    pub custom: Vec<CustomLauncher>,
}

/// A launcher the user added from the "Add Custom Launcher" window.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CustomLauncher {
    pub name: String,
    /// An image file or a program; empty for none.
    pub icon: String,
    /// What its popup lists, added from its tile's right-click menu.
    pub items: Vec<CustomItem>,
}

/// A program, shortcut or any other file in a custom launcher's popup, opened as if
/// double-clicked in Explorer.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CustomItem {
    pub name: String,
    pub path: String,
    /// An image file or a program whose icon is shown; empty for the item's own icon.
    pub icon: String,
}

/// Starts the dock-order key of every custom launcher.
pub const CUSTOM_KEY_PREFIX: &str = "custom:";

impl CustomLauncher {
    /// Its key in the saved dock order, unique since names are.
    pub fn key(&self) -> String {
        format!("{CUSTOM_KEY_PREFIX}{}", self.name.trim())
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct DockConfig {
    pub icon_size: u32,
    pub icon_shape: IconShape,
    pub vertical: bool,
    pub background: String,
    pub opacity: f64,
    pub always_on_top: bool,
    pub movable: bool,
    pub sortable: bool,
}

/// The tile drawn behind each launcher icon on the dock, in the settings window's order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum IconShape {
    Circle,
    /// A rounded square.
    Curve,
    /// No tile: the icon keeps its own transparent shape.
    #[default]
    Flat,
}

impl IconShape {
    pub const ALL: [IconShape; 3] = [IconShape::Circle, IconShape::Curve, IconShape::Flat];

    pub fn name(self) -> &'static str {
        match self {
            IconShape::Circle => "circle",
            IconShape::Curve => "curve",
            IconShape::Flat => "flat",
        }
    }
}

/// From its name, ignoring case; an unknown one logs and falls back to the default, rather than
/// failing the whole file.
impl<'de> Deserialize<'de> for IconShape {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(IconShape::ALL.into_iter().find(|shape| shape.name().eq_ignore_ascii_case(name.trim())).unwrap_or_else(|| {
            eprintln!("ignoring invalid icon_shape {name:?}, expected \"circle\", \"curve\" or \"flat\"");
            IconShape::default()
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct PopupConfig {
    pub columns: u32,
    pub landscape_columns: u32,
    pub rows: u32,
    /// Custom launchers' application icons only, not cover art.
    pub icon_shape: IconShape,
    pub background: String,
    pub opacity: f64,
    pub close_on_focus_loss: bool,
}

/// Default background of both the dock and the bubble.
const BACKGROUND: &str = "#1e1e24";

impl Default for Config {
    fn default() -> Self {
        Config {
            launchers: ["steam", "battlenet", "ea"].map(String::from).to_vec(),
            hidden_games: Vec::new(),
            autostart: false,
            dock: DockConfig::default(),
            popup: PopupConfig::default(),
            custom: Vec::new(),
        }
    }
}

impl Default for DockConfig {
    fn default() -> Self {
        DockConfig {
            icon_size: 48,
            icon_shape: IconShape::default(),
            vertical: false,
            background: BACKGROUND.into(),
            opacity: 0.9,
            always_on_top: false,
            movable: true,
            sortable: true,
        }
    }
}

impl Default for PopupConfig {
    fn default() -> Self {
        PopupConfig {
            columns: 6,
            landscape_columns: 3,
            rows: 3,
            icon_shape: IconShape::default(),
            background: BACKGROUND.into(),
            opacity: 1.0,
            close_on_focus_loss: true,
        }
    }
}

/// A `"#RRGGBB"` color with an opacity from 0.0 to 1.0, as `[red, green, blue, alpha]`. An
/// invalid color logs and falls back to the default background; opacity is clamped.
pub fn rgba(color: &str, opacity: f64) -> [u8; 4] {
    let [r, g, b] = parse_hex(color).unwrap_or_else(|| {
        eprintln!("ignoring invalid color {color:?}, expected \"#RRGGBB\"");
        parse_hex(BACKGROUND).unwrap()
    });
    let opacity = if opacity.is_nan() { 1.0 } else { opacity.clamp(0.0, 1.0) };
    [r, g, b, (opacity * 255.0).round() as u8]
}

/// Whether `color` is a valid `"#RRGGBB"`.
pub fn is_color(color: &str) -> bool {
    parse_hex(color).is_some()
}

fn parse_hex(color: &str) -> Option<[u8; 3]> {
    let hex = color.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// Reads the settings file, creating a commented default one if there is none. Fails soft:
/// an unreadable or invalid file yields the defaults.
pub fn load() -> Config {
    let Some(path) = path() else { return Config::default() };
    migrate_old_file(&path);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
            eprintln!("ignoring {}: {err}", path.display());
            Config::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            write_default(&path);
            Config::default()
        }
        Err(err) => {
            eprintln!("failed to read {}: {err}", path.display());
            Config::default()
        }
    }
}

pub fn path() -> Option<PathBuf> {
    Some(dir()?.join(FILE_NAME))
}

/// The app's folder in the OS config directory, holding `config.toml` and `state.toml`. The
/// folder's earlier name is renamed on first use so settings and state carry over; if that
/// fails, the old folder keeps being used.
pub fn dir() -> Option<PathBuf> {
    let base = crate::platform::config_dir()?;
    let dir = base.join(DIR_NAME);
    let old = base.join(OLD_DIR_NAME);
    if !dir.exists() && old.is_dir() {
        if let Err(err) = std::fs::rename(&old, &dir) {
            eprintln!("failed to rename {} to {}: {err}", old.display(), dir.display());
            return Some(old);
        }
    }
    Some(dir)
}

/// Renames `setting.toml` to `config.toml` when only the old file exists.
fn migrate_old_file(path: &Path) {
    let Some(old) = path.parent().map(|dir| dir.join(OLD_FILE_NAME)) else { return };
    if path.exists() || !old.exists() {
        return;
    }
    if let Err(err) = std::fs::rename(&old, path) {
        eprintln!("failed to rename {} to {}: {err}", old.display(), path.display());
    }
}

fn write_default(path: &Path) {
    if let Err(err) = write(path, &Config::default()) {
        eprintln!("failed to write {}: {err}", path.display());
    }
}

/// Replaces the settings file with `config`, comments included (the settings window).
pub fn save(config: &Config) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    write(&path, config)
}

fn write(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_defaults() {
        assert_eq!(toml::from_str::<Config>(&render(&Config::default())).unwrap(), Config::default());
    }

    #[test]
    fn rendered_file_round_trips() {
        let mut config = Config::default();
        config.launchers = vec!["ea".into(), "steam".into()];
        config.hidden_games = vec!["steam:570".into(), "odd \"id\"\\".into()];
        config.autostart = true;
        config.dock.icon_size = 64;
        config.dock.icon_shape = IconShape::Circle;
        config.dock.vertical = true;
        config.dock.background = "#ff8000".into();
        config.dock.opacity = 0.5;
        config.dock.movable = false;
        config.popup.columns = 4;
        config.popup.icon_shape = IconShape::Curve;
        config.popup.opacity = 1.0;
        config.popup.close_on_focus_loss = false;
        config.custom = vec![
            CustomLauncher {
                name: "My \"Launcher\"".into(),
                icon: r"C:\Games\launcher.exe".into(),
                items: vec![
                    CustomItem { name: "Game".into(), path: r"C:\Games\game.exe".into(), icon: String::new() },
                    CustomItem {
                        name: "Shortcut".into(),
                        path: r"C:\Users\Me\Desktop\Tool.lnk".into(),
                        icon: r"C:\Users\Me\Pictures\tool.png".into(),
                    },
                ],
            },
            CustomLauncher { name: "No icon".into(), icon: String::new(), items: Vec::new() },
        ];
        assert_eq!(toml::from_str::<Config>(&render(&config)).unwrap(), config);
    }

    #[test]
    fn floats_are_rounded() {
        assert_eq!(toml_float(0.8999999), "0.9");
        assert_eq!(toml_float(1.0), "1.0");
        assert_eq!(toml_float(f64::NAN), "1.0");
    }

    #[test]
    fn missing_keys_take_defaults() {
        let config: Config = toml::from_str("launchers = [\"ea\"]\n[popup]\ncolumns = 4\n").unwrap();
        assert_eq!(config.launchers, ["ea"]);
        assert_eq!(config.popup.columns, 4);
        assert_eq!(config.popup.rows, 3);
        assert_eq!(config.dock, DockConfig::default());
    }

    #[test]
    fn colors() {
        assert_eq!(rgba("#1e1e24", 0.9), [0x1e, 0x1e, 0x24, 0xe6]);
        assert_eq!(rgba("#FF8000", 2.0), [0xff, 0x80, 0x00, 0xff]);
        assert_eq!(rgba("red", -1.0), [0x1e, 0x1e, 0x24, 0]);
        assert_eq!(rgba("#12345", 1.0), [0x1e, 0x1e, 0x24, 0xff]);
    }

    #[test]
    fn integer_opacity() {
        let config: Config = toml::from_str("[popup]\nopacity = 1\n").unwrap();
        assert_eq!(config.popup.opacity, 1.0);
    }

    /// Keys of removed settings are ignored, so older files still load.
    #[test]
    fn removed_keys_are_ignored() {
        let old = "[dock]\nreserve_edge = true\nraise_with_start_menu = false\nraise_key = \"Ctrl+Alt+D\"\n";
        assert_eq!(toml::from_str::<Config>(old).unwrap(), Config::default());
    }

    #[test]
    fn icon_shapes() {
        let shape = |text: &str| toml::from_str::<Config>(&format!("[dock]\nicon_shape = {text:?}\n")).unwrap().dock.icon_shape;
        assert_eq!(shape("curve"), IconShape::Curve);
        assert_eq!(shape(" Circle "), IconShape::Circle);
        assert_eq!(shape("hexagon"), IconShape::Flat);
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    }
}
