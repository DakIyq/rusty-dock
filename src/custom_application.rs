//! The "Add Application" window, opened by right-clicking a custom launcher's popup: a name,
//! the program (or shortcut, or any file) its tile opens and an optional icon. "Save" adds it
//! to that launcher's `[[custom.items]]` in config.toml.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use slint::{ComponentHandle, Image};

use crate::config::{self, Config, CustomItem};
use crate::custom_launcher::{ICON_FILTER, closed, load_icon};
use crate::{CustomApplicationWindow, platform, providers};

/// The file dialog's filter for the application: pairs of label and patterns, each
/// NUL-terminated.
const APP_FILTER: &str = "Programs and shortcuts (*.exe;*.lnk;*.url;*.bat;*.cmd)\0*.exe;*.lnk;*.url;*.bat;*.cmd\0\
                          All files (*.*)\0*.*\0";

/// Created on first use, since most sessions never open it.
#[derive(Clone)]
pub struct CustomApplicationDialog {
    window: Rc<RefCell<Option<CustomApplicationWindow>>>,
    /// Whether the window is open, so reopening it for the same launcher keeps unsaved edits.
    shown: Rc<Cell<bool>>,
    /// Key of the custom launcher it adds to (`CustomLauncher::key`).
    launcher_key: Rc<RefCell<String>>,
    /// Index in that launcher's `items` of the application being edited; `None` when adding.
    editing: Rc<Cell<Option<usize>>>,
    /// Set while a file dialog is open, so Browse doesn't stack a second one.
    browsing: Arc<AtomicBool>,
    /// Runs with the launcher's key and the saved settings once an application was added or
    /// changed.
    on_added: Rc<dyn Fn(&str, &Config)>,
}

impl CustomApplicationDialog {
    pub fn new(on_added: impl Fn(&str, &Config) + 'static) -> Self {
        CustomApplicationDialog {
            window: Rc::default(),
            shown: Rc::default(),
            launcher_key: Rc::default(),
            editing: Rc::default(),
            browsing: Arc::default(),
            on_added: Rc::new(on_added),
        }
    }

    /// Shows the window for adding to the custom launcher `key` (display name `name`),
    /// emptied unless it is already open for adding to that launcher.
    pub fn open(&self, key: &str, name: &str) {
        self.show(key, name, None, &CustomItem::default());
    }

    /// Shows the window for changing `item`, at `index` in the `items` of the custom launcher
    /// `key` (display name `name`), filled with it unless already open for it.
    pub fn edit(&self, key: &str, name: &str, index: usize, item: &CustomItem) {
        self.show(key, name, Some(index), item);
    }

    /// Opening for another launcher or application, or after closing, refills the window;
    /// reopening it for the same one keeps unsaved edits.
    fn show(&self, key: &str, name: &str, editing: Option<usize>, item: &CustomItem) {
        let mut slot = self.window.borrow_mut();
        let window = slot.get_or_insert_with(|| self.create());
        if !self.shown.get() || *self.launcher_key.borrow() != key || self.editing.get() != editing {
            window.set_app_name(item.name.trim().into());
            window.set_app_path(item.path.trim().into());
            window.set_app_icon_path(item.icon.trim().into());
            window.set_app_icon_preview(Image::default());
            window.set_error("".into());
            show_preview(window);
            *self.launcher_key.borrow_mut() = key.to_string();
            self.editing.set(editing);
            self.shown.set(true);
        }
        window.set_launcher_name(name.into());
        window.set_editing(editing.is_some());
        window.show().unwrap();
    }

    pub fn hide(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.hide().unwrap();
        }
    }

    fn create(&self) -> CustomApplicationWindow {
        let window = CustomApplicationWindow::new().unwrap();

        {
            let weak = window.as_weak();
            let browsing = self.browsing.clone();
            window.on_browse_app(move || {
                browse(&browsing, "Choose an application", APP_FILTER, weak.clone(), |window, path| {
                    window.set_app_path(path.to_string_lossy().as_ref().into());
                    // A name is suggested from the file, unless one was typed already.
                    if window.get_app_name().trim().is_empty() {
                        window.set_app_name(providers::file_title(path).into());
                    }
                    show_preview(window);
                });
            });
        }
        {
            let weak = window.as_weak();
            let browsing = self.browsing.clone();
            window.on_browse_icon(move || {
                browse(&browsing, "Choose an icon", ICON_FILTER, weak.clone(), |window, path| {
                    window.set_app_icon_path(path.to_string_lossy().as_ref().into());
                    show_preview(window);
                });
            });
        }
        {
            let weak = window.as_weak();
            window.on_app_path_edited(move || show_preview(&weak.unwrap()));
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
            let key = self.launcher_key.clone();
            let editing = self.editing.clone();
            let on_added = self.on_added.clone();
            window.on_add(move || {
                let window = weak.unwrap();
                let key = key.borrow().clone();
                match read(&window).and_then(|item| save(&key, editing.get(), item)) {
                    Ok(config) => {
                        window.hide().unwrap();
                        closed(&shown);
                        on_added(&key, &config);
                    }
                    Err(message) => window.set_error(message.into()),
                }
            });
        }

        window
    }
}

/// Opens a file dialog on a worker thread (its modal loop can't run inside a Slint callback)
/// and hands the picked file to `picked` on the UI thread.
fn browse(
    browsing: &Arc<AtomicBool>,
    title: &'static str,
    filter: &'static str,
    weak: slint::Weak<CustomApplicationWindow>,
    picked: fn(&CustomApplicationWindow, &Path),
) {
    if browsing.swap(true, Ordering::SeqCst) {
        return;
    }
    let browsing = browsing.clone();
    std::thread::spawn(move || {
        let path = platform::pick_file(title, filter);
        browsing.store(false, Ordering::SeqCst);
        let Some(path) = path else { return };
        let _ = weak.upgrade_in_event_loop(move |window| picked(&window, &path));
    });
}

/// The chosen icon, else the application's own (a program's icon, or an image file).
fn show_preview(window: &CustomApplicationWindow) {
    let scale = window.window().scale_factor();
    let icon = window.get_app_icon_path().trim().to_string();
    let source = if icon.is_empty() { window.get_app_path().trim().to_string() } else { icon };
    let preview = if source.is_empty() { None } else { load_icon(Path::new(&source), scale) };
    window.set_app_icon_preview(preview.unwrap_or_default());
}

/// The window's values; fails with a message when the application is missing or the chosen
/// icon can't be read. An empty name becomes the file's name.
fn read(window: &CustomApplicationWindow) -> Result<CustomItem, String> {
    let path = window.get_app_path().trim().to_string();
    if path.is_empty() {
        return Err("Choose the application to open.".into());
    }
    if !Path::new(&path).exists() {
        return Err(format!("{path} doesn't exist."));
    }
    let name = match window.get_app_name().trim() {
        "" => providers::file_title(Path::new(&path)),
        name => name.to_string(),
    };
    let icon = window.get_app_icon_path().trim().to_string();
    if !icon.is_empty() && load_icon(&PathBuf::from(&icon), 1.0).is_none() {
        return Err(format!("Couldn't read an icon from {icon}."));
    }
    Ok(CustomItem { name, path, icon })
}

/// Adds `item` to the custom launcher `key` in config.toml, or with `editing` replaces the
/// application at that index (reloaded first, so other settings are kept), and returns the
/// saved settings.
fn save(key: &str, editing: Option<usize>, item: CustomItem) -> Result<Config, String> {
    let mut config = config::load();
    let Some(launcher) = config.custom.iter_mut().find(|custom| custom.key().eq_ignore_ascii_case(key)) else {
        return Err("This launcher is no longer in the settings.".into());
    };
    match editing {
        Some(index) => match launcher.items.get_mut(index) {
            Some(slot) => *slot = item,
            None => return Err("This application is no longer in the settings.".into()),
        },
        None => launcher.items.push(item),
    }
    config::save(&config).map_err(|err| format!("Couldn't save the application: {err}"))?;
    Ok(config)
}
