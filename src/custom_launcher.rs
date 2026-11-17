//! The "Add Custom Launcher" window, opened from the app menu: a name and an icon for a
//! launcher the user defines. "Save" adds it to config.toml's `[[custom]]` list. Opened by a
//! custom launcher's "Edit" instead, it changes that entry.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use slint::{ComponentHandle, Image};

use crate::config::{self, Config, CustomLauncher};
use crate::{CustomLauncherWindow, platform};

/// The icon preview's size in logical pixels; keep in sync with custom_launcher.slint.
const PREVIEW_SIZE: f32 = 64.0;

/// Created on first use, since most sessions never open it.
#[derive(Clone)]
pub struct CustomLauncherDialog {
    window: Rc<RefCell<Option<CustomLauncherWindow>>>,
    /// Whether the window is open, so reopening it from the menu keeps unsaved edits.
    shown: Rc<Cell<bool>>,
    /// Set while the file dialog is open, so Browse doesn't stack a second one.
    browsing: Arc<AtomicBool>,
    /// Key (`CustomLauncher::key`) of the launcher being edited; `None` when adding one.
    editing: Rc<RefCell<Option<String>>>,
    /// Runs with the saved settings once a launcher was added or changed, to update the dock.
    on_added: Rc<dyn Fn(&Config)>,
}

impl CustomLauncherDialog {
    pub fn new(on_added: impl Fn(&Config) + 'static) -> Self {
        CustomLauncherDialog {
            window: Rc::default(),
            shown: Rc::default(),
            browsing: Arc::default(),
            editing: Rc::default(),
            on_added: Rc::new(on_added),
        }
    }

    /// Shows the window for adding a launcher, emptied unless it is already open for that.
    pub fn open(&self) {
        let mut slot = self.window.borrow_mut();
        let window = slot.get_or_insert_with(|| self.create());
        if !self.shown.get() || self.editing.borrow().is_some() {
            fill(window, "", "");
            *self.editing.borrow_mut() = None;
            self.shown.set(true);
        }
        window.set_editing(false);
        window.show().unwrap();
    }

    /// Shows the window for changing the custom launcher `key`, filled from config.toml unless
    /// it is already open for that launcher (keeping unsaved edits).
    pub fn edit(&self, key: &str) {
        let config = config::load();
        let Some(launcher) = config.custom.iter().find(|custom| custom.key().eq_ignore_ascii_case(key)) else {
            eprintln!("custom launcher {key:?} is gone from the settings");
            return;
        };
        let mut slot = self.window.borrow_mut();
        let window = slot.get_or_insert_with(|| self.create());
        if !self.shown.get() || self.editing.borrow().as_deref() != Some(key) {
            fill(window, launcher.name.trim(), launcher.icon.trim());
            *self.editing.borrow_mut() = Some(key.to_string());
            self.shown.set(true);
        }
        window.set_editing(true);
        window.show().unwrap();
    }

    pub fn hide(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.hide().unwrap();
        }
    }

    fn create(&self) -> CustomLauncherWindow {
        let window = CustomLauncherWindow::new().unwrap();

        {
            let weak = window.as_weak();
            let browsing = self.browsing.clone();
            window.on_browse_icon(move || {
                if browsing.swap(true, Ordering::SeqCst) {
                    return;
                }
                let weak = weak.clone();
                let browsing = browsing.clone();
                // The file dialog runs its own modal loop; on a worker thread it can't reenter
                // Slint's event loop from inside this callback.
                std::thread::spawn(move || {
                    let picked = platform::pick_file("Choose an icon", ICON_FILTER);
                    browsing.store(false, Ordering::SeqCst);
                    let Some(path) = picked else { return };
                    let _ = weak.upgrade_in_event_loop(move |window| {
                        window.set_launcher_icon_path(path.to_string_lossy().as_ref().into());
                        show_preview(&window, &path);
                    });
                });
            });
        }

        {
            let weak = window.as_weak();
            let shown = self.shown.clone();
            window.on_cancel(move || {
                weak.unwrap().hide().unwrap();
                closed(&shown);
            });
        }
        {
            let shown = self.shown.clone();
            window.window().on_close_requested(move || {
                closed(&shown);
                slint::CloseRequestResponse::HideWindow
            });
        }

        {
            let weak = window.as_weak();
            let shown = self.shown.clone();
            let editing = self.editing.clone();
            let on_added = self.on_added.clone();
            window.on_add(move || {
                let window = weak.unwrap();
                let editing = editing.borrow().clone();
                match read(&window).and_then(|launcher| save(editing.as_deref(), launcher)) {
                    Ok(config) => {
                        window.hide().unwrap();
                        closed(&shown);
                        on_added(&config);
                    }
                    Err(message) => window.set_error(message.into()),
                }
            });
        }

        window
    }
}

/// The file dialog's filter: pairs of label and patterns, each NUL-terminated.
pub const ICON_FILTER: &str = "Icons (*.png;*.jpg;*.jpeg;*.webp;*.exe)\0*.png;*.jpg;*.jpeg;*.webp;*.exe\0\
                           Images (*.png;*.jpg;*.jpeg;*.webp)\0*.png;*.jpg;*.jpeg;*.webp\0\
                           Programs (*.exe)\0*.exe\0\
                           All files (*.*)\0*.*\0";

/// The window was closed: the next `open` empties it, and memory is trimmed once it's gone.
pub fn closed(shown: &Cell<bool>) {
    shown.set(false);
    slint::Timer::single_shot(crate::TRIM_DELAY, platform::trim_working_set);
}

/// Sets the window's fields: a launcher's name and icon path (empty for none).
fn fill(window: &CustomLauncherWindow, name: &str, icon: &str) {
    window.set_launcher_name(name.into());
    window.set_launcher_icon_path(icon.into());
    window.set_error("".into());
    if icon.is_empty() {
        window.set_launcher_icon_preview(Image::default());
    } else {
        show_preview(window, Path::new(icon));
    }
}

/// Adds the launcher to config.toml's `[[custom]]` list, or with `editing` changes the name and
/// icon of that one, keeping its applications (reloaded first, so other settings are kept), and
/// returns the saved settings; fails with a message when the name is taken.
fn save(editing: Option<&str>, (name, icon): (String, Option<PathBuf>)) -> Result<Config, String> {
    let mut config = config::load();
    let icon = icon.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
    let edited = match editing {
        Some(old) => Some(
            config
                .custom
                .iter()
                .position(|custom| custom.key().eq_ignore_ascii_case(old))
                .ok_or("This launcher is no longer in the settings.")?,
        ),
        None => None,
    };
    let key = CustomLauncher { name: name.clone(), ..Default::default() }.key();
    let taken = config
        .custom
        .iter()
        .enumerate()
        .any(|(i, other)| Some(i) != edited && other.key().eq_ignore_ascii_case(&key));
    if taken {
        return Err(format!("There is already a launcher named {name}."));
    }
    match edited {
        Some(i) => {
            let old_key = config.custom[i].key();
            config.custom[i].name = name;
            config.custom[i].icon = icon;
            if old_key != key {
                renamed(&mut config, &old_key, &key);
            }
        }
        None => config.custom.push(CustomLauncher { name, icon, items: Vec::new() }),
    }
    config::save(&config).map_err(|err| format!("Couldn't save the launcher: {err}"))?;
    Ok(config)
}

/// Carries a renamed launcher's saved dock position (state.toml) and hidden applications (ids
/// start with its key) over to its new key.
fn renamed(config: &mut Config, old_key: &str, new_key: &str) {
    let prefix = format!("{old_key}:");
    for id in &mut config.hidden_games {
        if let Some(rest) = id.trim().strip_prefix(&prefix) {
            *id = format!("{new_key}:{rest}");
        }
    }
    crate::state::update(|state| {
        for key in &mut state.dock.order {
            if key.eq_ignore_ascii_case(old_key) {
                *key = new_key.to_string();
            }
        }
    });
}

fn show_preview(window: &CustomLauncherWindow, path: &Path) {
    let scale = window.window().scale_factor();
    window.set_launcher_icon_preview(load_icon(path, scale).unwrap_or_default());
}

/// The window's name and icon path; fails with a message when the name is empty or the icon
/// can't be read. No icon is fine: the dock then shows the name's initials.
fn read(window: &CustomLauncherWindow) -> Result<(String, Option<PathBuf>), String> {
    let name = window.get_launcher_name().trim().to_string();
    if name.is_empty() {
        return Err("Give the launcher a name.".into());
    }
    let icon = window.get_launcher_icon_path().trim().to_string();
    if icon.is_empty() {
        return Ok((name, None));
    }
    let icon = PathBuf::from(icon);
    if load_icon(&icon, 1.0).is_none() {
        return Err(format!("Couldn't read an icon from {}.", icon.display()));
    }
    Ok((name, Some(icon)))
}

/// An image file, or a program's own icon, scaled to the preview's size in physical pixels.
pub fn load_icon(path: &Path, scale: f32) -> Option<Image> {
    let size = (PREVIEW_SIZE * scale).round().max(1.0) as u32;
    crate::load_icon(path, size, size)
}
