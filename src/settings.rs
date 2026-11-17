//! The settings window, opened from the app menu. It edits `config.toml` and applies it to the
//! running dock on save.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use slint::{ComponentHandle, Timer};

use crate::config::{self, Config, IconShape};
use crate::{SettingsWindow, platform};

/// Created on first use, since most sessions never open it.
#[derive(Clone)]
pub struct Settings {
    window: Rc<RefCell<Option<SettingsWindow>>>,
    /// Whether the window is open, so reopening it from the menu keeps unsaved edits.
    shown: Rc<Cell<bool>>,
    /// Run with the saved settings, to apply them.
    on_saved: Rc<dyn Fn(&Config)>,
}

impl Settings {
    pub fn new(on_saved: impl Fn(&Config) + 'static) -> Self {
        Settings { window: Rc::default(), shown: Rc::default(), on_saved: Rc::new(on_saved) }
    }

    /// Shows the window, filled from `config.toml` unless it is already open.
    pub fn open(&self) {
        let mut slot = self.window.borrow_mut();
        let window = slot.get_or_insert_with(|| self.create());
        if !self.shown.get() {
            fill(window, &config::load());
            self.shown.set(true);
        }
        window.show().unwrap();
    }

    pub fn hide(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.hide().unwrap();
        }
    }

    fn create(&self) -> SettingsWindow {
        let window = SettingsWindow::new().unwrap();

        window.on_preview_color(|hex, opacity| {
            if config::is_color(&hex) {
                crate::color(&hex, opacity as f64)
            } else {
                slint::Color::from_argb_u8(0, 0, 0, 0)
            }
        });

        window.on_open_file(|| {
            if let Some(path) = config::path() {
                platform::edit_file(&path);
            }
        });

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
            let on_saved = self.on_saved.clone();
            window.on_save(move || {
                let window = weak.unwrap();
                // Reloaded, so settings the window doesn't show (launchers, hidden games) are kept, even if
                // edited in the file meanwhile.
                let mut config = config::load();
                let result = read(&window, &mut config).and_then(|()| {
                    config::save(&config).map_err(|err| format!("Couldn't save the settings: {err}"))
                });
                match result {
                    Ok(()) => {
                        window.hide().unwrap();
                        closed(&shown);
                        on_saved(&config);
                    }
                    Err(message) => window.set_error(message.into()),
                }
            });
        }

        window
    }
}

fn closed(shown: &Cell<bool>) {
    shown.set(false);
    Timer::single_shot(crate::TRIM_DELAY, platform::trim_working_set);
}

fn fill(window: &SettingsWindow, config: &Config) {
    window.set_autostart(config.autostart);

    let dock = &config.dock;
    window.set_dock_icon_size(dock.icon_size.clamp(16, 256) as i32);
    window.set_dock_icon_shape(shape_index(dock.icon_shape));
    window.set_dock_vertical(dock.vertical);
    window.set_dock_background(dock.background.as_str().into());
    window.set_dock_opacity(opacity(dock.opacity));
    window.set_dock_always_on_top(dock.always_on_top);
    window.set_dock_movable(dock.movable);
    window.set_dock_sortable(dock.sortable);

    let popup = &config.popup;
    window.set_popup_columns(popup.columns.clamp(1, 20) as i32);
    window.set_popup_landscape_columns(popup.landscape_columns.clamp(1, 20) as i32);
    window.set_popup_rows(popup.rows.clamp(1, 20) as i32);
    window.set_popup_icon_shape(shape_index(popup.icon_shape));
    window.set_popup_background(popup.background.as_str().into());
    window.set_popup_opacity(opacity(popup.opacity));
    window.set_popup_close_on_focus_loss(popup.close_on_focus_loss);

    window.set_error("".into());
}

/// Copies the window's values into `config`; fails with a message on an invalid color.
fn read(window: &SettingsWindow, config: &mut Config) -> Result<(), String> {
    let dock_background = valid_color("dock", &window.get_dock_background())?;
    let popup_background = valid_color("popup", &window.get_popup_background())?;

    config.autostart = window.get_autostart();

    let dock = &mut config.dock;
    dock.icon_size = window.get_dock_icon_size().clamp(16, 256) as u32;
    dock.icon_shape = shape_at(window.get_dock_icon_shape()).unwrap_or(dock.icon_shape);
    dock.vertical = window.get_dock_vertical();
    dock.background = dock_background;
    dock.opacity = window.get_dock_opacity() as f64;
    dock.always_on_top = window.get_dock_always_on_top();
    dock.movable = window.get_dock_movable();
    dock.sortable = window.get_dock_sortable();

    let popup = &mut config.popup;
    popup.columns = window.get_popup_columns().max(1) as u32;
    popup.landscape_columns = window.get_popup_landscape_columns().max(1) as u32;
    popup.rows = window.get_popup_rows().max(1) as u32;
    popup.icon_shape = shape_at(window.get_popup_icon_shape()).unwrap_or(popup.icon_shape);
    popup.background = popup_background;
    popup.opacity = window.get_popup_opacity() as f64;
    popup.close_on_focus_loss = window.get_popup_close_on_focus_loss();
    Ok(())
}

fn valid_color(which: &str, text: &str) -> Result<String, String> {
    let text = text.trim();
    if config::is_color(text) {
        Ok(text.to_string())
    } else {
        Err(format!("The {which} background {text:?} isn't a color like #1e1e24."))
    }
}

/// An "Icon type" `ComboBox` index, whose choices are in `IconShape::ALL`'s order.
fn shape_index(shape: IconShape) -> i32 {
    IconShape::ALL.iter().position(|&s| s == shape).unwrap_or(0) as i32
}

fn shape_at(index: i32) -> Option<IconShape> {
    IconShape::ALL.get(usize::try_from(index).ok()?).copied()
}

fn opacity(value: f64) -> f32 {
    if value.is_nan() { 1.0 } else { value.clamp(0.0, 1.0) as f32 }
}
