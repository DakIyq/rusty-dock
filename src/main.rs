// Release builds on Windows run as a GUI app with no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod game;
mod platform;
mod providers;
mod config;
mod custom_application;
mod custom_launcher;
mod settings;
mod state;

use std::cell::{Cell, OnceCell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
// use std::time::Instant;

use slint::{Image, Model, ModelRc, PhysicalPosition, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};

use config::Config;
use game::{Game, Launcher};

slint::include_modules!();

/// The name the user sees (tray tooltip); the exe and the config folder are `RustyDock`.
const APP_NAME: &str = "Rusty Dock";
/// Space between the dock and the games popup, in logical pixels.
const POPUP_GAP: f32 = 2.0;
/// Smallest space between the games popup and the edge of the screen it is shifted along, in
/// logical pixels.
const POPUP_EDGE_MARGIN: f32 = 2.0;
/// How long after applying the settings to pull the dock back on screen, once the window has
/// taken its new size (a larger icon size, or turning vertical, can push it past an edge).
const RECLAMP_DELAY: Duration = Duration::from_millis(100);
/// How often to check whether another app took focus while the popup is open.
const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(150);
// /// How long after the mouse leaves the dock and popup to send them behind other windows.
// const PUSH_BACK_DELAY: Duration = Duration::from_secs(3);
/// How long after startup or closing the popup to trim the working set, once repainting is done.
const TRIM_DELAY: Duration = Duration::from_secs(2);
/// How long after the last drag movement to save the dock's position, so a drag writes
/// `state.toml` once rather than on every mouse move.
const SAVE_POSITION_DELAY: Duration = Duration::from_millis(500);
/// Game tile sizes in logical pixels; keep in sync with `GamesPopup` in main.slint. Launcher
/// tiles are square, `[dock] icon_size` from the settings.
const PORTRAIT_TILE: (u32, u32) = (72, 108);
const LANDSCAPE_TILE: (u32, u32) = (160, 90);
/// Square icon tiles of custom launchers' applications.
const ICON_TILE: (u32, u32) = (56, 56);
/// Corner radius of game tiles in logical pixels; keep in sync with `GamesPopup` in main.slint.
const GAME_TILE_RADIUS: f32 = 8.0;

fn main() {
    // Held until `main` returns.
    let Some(_instance) = platform::InstanceLock::acquire() else { return };
    let dock = Dock::new().unwrap();
    let popup = GamesPopup::new().unwrap();
    let app = Rc::new(App::new(&dock, &popup));
    app.apply(&config::load());
    // Tiles are drawn at this scale until the window exists on its monitor.
    let scale = dock.window().scale_factor();

    {
        let app = app.clone();
        dock.on_launcher_clicked(move |position, center| app.toggle_popup(position as usize, center));
    }
    {
        let app = app.clone();
        dock.on_launcher_moved(move |from, to| app.move_launcher(from as usize, to as usize));
    }
    {
        let app = app.clone();
        popup.on_launch(move |id| app.launch(&id));
    }
    {
        let app = app.clone();
        popup.on_dismiss(move || app.close_popup());
    }

    // With no taskbar button, the tray icon and the dock's right-click menu are how the user
    // quits. Both quit through the event loop, so the windows are hidden cleanly below.
    // Saving the settings applies them to the running dock.
    let settings = {
        let app = app.clone();
        settings::Settings::new(move |config| app.apply(config))
    };
    // An added launcher shows up on the dock right away.
    let custom_launcher = {
        let app = app.clone();
        custom_launcher::CustomLauncherDialog::new(move |config| app.apply(config))
    };
    let custom_application = {
        let app = app.clone();
        custom_application::CustomApplicationDialog::new(move |key, config| app.custom_items_changed(key, config))
    };
    let tray = {
        let settings = settings.clone();
        let custom_launcher = custom_launcher.clone();
        let custom_application = custom_application.clone();
        let app = app.clone();
        platform::Tray::new(APP_NAME, app_menu, move |command| match command {
            platform::MenuCommand::AddCustomLauncher => custom_launcher.open(),
            platform::MenuCommand::EditSettings => settings.open(),
            // Also picks up hand edits to config.toml and newly installed games.
            platform::MenuCommand::Refresh => app.apply(&config::load()),
            platform::MenuCommand::Quit => {
                let _ = slint::quit_event_loop();
            }
            platform::MenuCommand::RemoveLauncher(position) => app.remove_launcher(position),
            platform::MenuCommand::ToggleLauncher(key) => app.toggle_launcher(key),
            platform::MenuCommand::AddCustomItem(position) => {
                let launcher = app.order.borrow().get(position).map(|&index| {
                    let launchers = app.launchers.borrow();
                    (launchers[index].key.clone(), launchers[index].name.clone())
                });
                if let Some((key, name)) = launcher {
                    custom_application.open(&key, &name);
                }
            }
            platform::MenuCommand::RemoveCustomItem(position, index) => app.remove_custom_item(position, index),
            platform::MenuCommand::EditCustomItem(position, index) => {
                let Some((key, config)) = app.checked_custom_item(position, index) else { return };
                let Some(custom) = config.custom.iter().find(|custom| custom.key().eq_ignore_ascii_case(&key)) else {
                    return;
                };
                custom_application.edit(&key, custom.name.trim(), index, &custom.items[index]);
            }
            platform::MenuCommand::EditCustomLauncher(position) => {
                if let Some(key) = app.key_at(position) {
                    custom_launcher.edit(&key);
                }
            }
        })
    };
    {
        let app = app.clone();
        dock.on_show_menu(move || {
            app.close_popup();
            platform::show_app_menu();
        });
    }
    {
        let app = app.clone();
        dock.on_launcher_menu(move |position| {
            app.close_popup();
            let position = position as usize;
            let launcher = app.order.borrow().get(position).map(|&index| {
                let launchers = app.launchers.borrow();
                (launchers[index].name.clone(), launchers[index].is_custom())
            });
            if let Some((name, custom)) = launcher {
                platform::show_launcher_menu(position, &name, custom);
            }
        });
    }
    {
        let app = app.clone();
        // Only custom launchers' popups have a menu; it keeps the popup open (the menu belongs
        // to this process, so the focus poll doesn't count it as switching away).
        popup.on_show_menu(move || {
            let Some(position) = app.open.get() else { return };
            if app.custom_at(position) {
                platform::show_custom_popup_menu(position, None);
            }
        });
    }
    {
        let app = app.clone();
        popup.on_item_menu(move |id| {
            let Some(position) = app.open.get() else { return };
            if !app.custom_at(position) {
                return;
            }
            // A custom application's id ends with its index in the launcher's `items`.
            let index = id.rsplit_once(':').and_then(|(_, index)| index.parse::<usize>().ok());
            let title = app.launchers.borrow().iter().flat_map(|l| &l.games).find(|game| game.id == id.as_str()).map(|game| game.title.clone());
            match (index, title) {
                (Some(index), Some(title)) => platform::show_custom_popup_menu(position, Some((index, &title))),
                _ => platform::show_custom_popup_menu(position, None),
            }
        });
    }

    // Close the popup once the user clicks or switches to another app. Like the taskbar, the
    // dock rises above every window while the Start menu is open (e.g. after pressing Win)
    // and stays there until another app takes the foreground.
    let focus_watch = Timer::default();
    {
        let app = app.clone();
        // let last_hover = Cell::new(Instant::now());
        // let pushed_back = Cell::new(false);
        focus_watch.start(TimerMode::Repeated, FOCUS_POLL_INTERVAL, move || {
            // Always on top, it is already above Start's neighbours and must not be dropped back.
            if app.always_on_top.get() {
                app.raised.set(false);
            } else if platform::start_menu_in_foreground() {
                if !app.raised.get() {
                    platform::set_topmost(app.dock.unwrap().window(), true);
                    app.raised.set(true);
                }
            } else if app.raised.get() && platform::other_app_in_foreground() {
                platform::drop_behind_foreground(app.dock.unwrap().window());
                platform::drop_behind_foreground(app.popup.unwrap().window());
                app.raised.set(false);
            }
            if app.open.get().is_some() {
                if platform::app_in_foreground() {
                    app.popup_had_focus.set(true);
                } else if app.close_on_focus_loss.get() && app.popup_had_focus.get() && platform::other_app_in_foreground() {
                    app.close_popup();
                }
            }
            // Auto-hide: once the mouse has been away for `PUSH_BACK_DELAY`, the popup closes and
            // the dock goes behind other windows.
            // if app.always_on_top.get() || app.raised.get() || platform::cursor_over_app() {
            //     last_hover.set(Instant::now());
            //     pushed_back.set(false);
            // } else if !pushed_back.get() && last_hover.get().elapsed() >= PUSH_BACK_DELAY {
            //     if app.open.get().is_some() {
            //         // Hiding the active popup can activate the dock, which would raise it again,
            //         // so the dock goes back on the next tick.
            //         app.close_popup();
            //     } else {
            //         platform::send_to_back(app.dock.unwrap().window());
            //         pushed_back.set(true);
            //     }
            // }
        });
    }

    let save_position = Timer::default();
    {
        let app = app.clone();
        dock.on_move_window(move |dx, dy| {
            // Unmovable, drags are ignored and the dock stays where it was last saved (or centered).
            if !app.movable.get() {
                return;
            }
            // Restarted by every movement, so it fires once the drag has ended.
            let moved = app.dock.clone();
            save_position.start(TimerMode::SingleShot, SAVE_POSITION_DELAY, move || {
                let pos = moved.unwrap().window().position();
                state::update(|state| {
                    state.dock.x = Some(pos.x);
                    state.dock.y = Some(pos.y);
                });
            });
            // The popup would be left behind, so close it while dragging.
            app.close_popup();
            let dock = app.dock.unwrap();
            let window = dock.window();
            // dx/dy are logical pixels; the window position is physical.
            let scale = window.scale_factor();
            let pos = window.position();
            let (x, y) = keep_on_screen(
                pos.x + (dx * scale) as i32,
                pos.y + (dy * scale) as i32,
                window.size(),
            );
            window.set_position(PhysicalPosition::new(x, y));
        });
    }

    // Show first so the window has its real size, then place it once it exists.
    dock.show().unwrap();
    {
        let app = app.clone();
        let saved = state::load().dock;
        when_created(dock.as_weak(), move |dock| {
            // The real scale factor may only be known once the window exists on its monitor.
            // Icons sit in fixed-size tiles, so replacing them doesn't change the dock's size.
            if dock.window().scale_factor() != scale {
                app.refresh_tiles();
            }
            platform::hide_from_taskbar(dock.window());
            if app.always_on_top.get() {
                platform::set_topmost(dock.window(), true);
            }
            // Where the user last dragged it; kept on screen in case the monitors changed since.
            match (saved.x, saved.y) {
                (Some(x), Some(y)) => {
                    let (x, y) = keep_on_screen(x, y, dock.window().size());
                    dock.window().set_position(PhysicalPosition::new(x, y));
                }
                _ => place_default(dock.window(), app.vertical.get()),
            }
        });
    }
    Timer::single_shot(TRIM_DELAY, platform::trim_working_set);
    slint::run_event_loop().unwrap();
    settings.hide();
    custom_launcher.hide();
    custom_application.hide();
    popup.hide().unwrap();
    dock.hide().unwrap();
    drop(tray);
}

/// The dock's state, shared by its callbacks. `apply` fills it from the settings at startup,
/// whenever they are saved and on Refresh, so nothing needs a restart.
struct App {
    dock: slint::Weak<Dock>,
    popup: slint::Weak<GamesPopup>,
    /// The installed launchers, as last scanned.
    launchers: RefCell<Vec<Launcher>>,
    /// Each launcher's game tiles, loaded the first time its popup opens.
    game_models: RefCell<Vec<OnceCell<ModelRc<DockItem>>>>,
    /// Index into `launchers` of the launcher at each dock position; dragging rearranges it.
    order: RefCell<Vec<usize>>,
    /// The dock's tiles, in dock order.
    items: Rc<VecModel<DockItem>>,
    /// Dock position of the launcher whose popup is open.
    open: Cell<Option<usize>>,
    /// The `center` the popup was last opened at, to reopen it after adding a custom item.
    popup_center: Cell<f32>,
    /// Whether we have held the foreground since the popup opened. Opened from a closing Start
    /// menu, the popup may start out without it, which doesn't mean the user switched away.
    popup_had_focus: Cell<bool>,
    /// Whether the dock is above every window because of the Start menu.
    raised: Cell<bool>,
    /// Settings the callbacks read, as last applied. `icon_size` is in logical pixels.
    icon_size: Cell<u32>,
    icon_shape: Cell<config::IconShape>,
    /// Shape of custom launchers' application icons in the popup.
    popup_icon_shape: Cell<config::IconShape>,
    vertical: Cell<bool>,
    movable: Cell<bool>,
    always_on_top: Cell<bool>,
    close_on_focus_loss: Cell<bool>,
}

impl App {
    fn new(dock: &Dock, popup: &GamesPopup) -> App {
        let items: Rc<VecModel<DockItem>> = Rc::new(VecModel::default());
        dock.set_launchers(ModelRc::from(items.clone()));
        App {
            dock: dock.as_weak(),
            popup: popup.as_weak(),
            launchers: RefCell::default(),
            game_models: RefCell::default(),
            order: RefCell::default(),
            items,
            open: Cell::new(None),
            popup_center: Cell::new(0.0),
            popup_had_focus: Cell::new(false),
            raised: Cell::new(false),
            icon_size: Cell::new(48),
            icon_shape: Cell::new(config::IconShape::default()),
            popup_icon_shape: Cell::new(config::IconShape::default()),
            vertical: Cell::new(false),
            movable: Cell::new(true),
            always_on_top: Cell::new(false),
            close_on_focus_loss: Cell::new(true),
        }
    }

    /// Applies the settings to the dock and the popup. The launchers are scanned again, which
    /// also picks up newly enabled launchers and changed `hidden_games`.
    fn apply(&self, config: &Config) {
        let (dock, popup) = (self.dock.unwrap(), self.popup.unwrap());
        // Its tiles, size and position may all change.
        self.close_popup();
        platform::set_autostart(config.autostart);

        // Bounded so a typo can't make the dock vanish or cover the screen.
        self.icon_size.set(config.dock.icon_size.clamp(16, 256));
        dock.set_icon_size(self.icon_size.get() as f32);
        self.icon_shape.set(config.dock.icon_shape);
        dock.set_icon_shape(slint_shape(config.dock.icon_shape));
        self.vertical.set(config.dock.vertical);
        dock.set_vertical(config.dock.vertical);
        dock.set_panel_color(color(&config.dock.background, config.dock.opacity));
        // Off, the tiles can't be dragged; the order last saved still applies.
        dock.set_sortable(config.dock.sortable);
        self.movable.set(config.dock.movable);
        self.always_on_top.set(config.dock.always_on_top);
        // Does nothing before the native window exists; `main` sets it again once it does.
        platform::set_topmost(dock.window(), config.dock.always_on_top);

        popup.set_portrait_columns(count(config.popup.columns));
        popup.set_landscape_columns(count(config.popup.landscape_columns));
        popup.set_max_rows(count(config.popup.rows));
        popup.set_bubble_color(color(&config.popup.background, config.popup.opacity));
        // Only custom launchers' application icons; covers keep GAME_TILE_RADIUS.
        self.popup_icon_shape.set(config.popup.icon_shape);
        popup.set_icon_shape(slint_shape(config.popup.icon_shape));
        self.close_on_focus_loss.set(config.popup.close_on_focus_loss);

        let mut launchers = providers::scan_all(&config.launchers, &config.custom, &config.hidden_games);
        // In the order the user last dragged them into; ones missing from it (newly installed or
        // enabled) keep the settings' order after the rest. The sort is stable.
        let saved = state::load().dock.order;
        launchers.sort_by_key(|launcher| {
            saved.iter().position(|key| key.eq_ignore_ascii_case(&launcher.key)).unwrap_or(usize::MAX)
        });
        *self.game_models.borrow_mut() = launchers.iter().map(|_| OnceCell::new()).collect();
        *self.order.borrow_mut() = (0..launchers.len()).collect();
        *self.launchers.borrow_mut() = launchers;
        self.refresh_tiles();

        // The dock keeps its top-left corner while its size changes, so a column may now reach
        // below the screen. At startup `main` places it once the window exists.
        if platform::has_native_window(dock.window()) {
            let weak = self.dock.clone();
            Timer::single_shot(RECLAMP_DELAY, move || {
                let Some(dock) = weak.upgrade() else { return };
                let window = dock.window();
                let pos = window.position();
                let (x, y) = keep_on_screen(pos.x, pos.y, window.size());
                if (x, y) != (pos.x, pos.y) {
                    window.set_position(PhysicalPosition::new(x, y));
                }
            });
        }
    }

    /// Redraws the dock's tiles at the current icon size and scale factor.
    fn refresh_tiles(&self) {
        let scale = self.dock.unwrap().window().scale_factor();
        let size = self.icon_size.get();
        let radius = icon_radius(self.icon_shape.get(), size) * scale;
        let rows = launcher_rows(&self.launchers.borrow(), &self.order.borrow(), size, radius, scale);
        self.items.set_vec(rows);
    }

    /// Hides the games popup, if one is open.
    fn close_popup(&self) {
        if self.open.take().is_none() {
            return;
        }
        self.dock.unwrap().set_active_index(-1);
        self.popup.unwrap().hide().unwrap();
        Timer::single_shot(TRIM_DELAY, platform::trim_working_set);
    }

    /// Opens the games popup of the launcher at `position`, or closes it if it is already open;
    /// `center` is the launcher's center along the dock (x, or y when vertical), in logical pixels.
    fn toggle_popup(&self, position: usize, center: f32) {
        if self.open.get() == Some(position) {
            self.close_popup();
            return;
        }
        let Some(&index) = self.order.borrow().get(position) else { return };
        let (dock, popup) = (self.dock.unwrap(), self.popup.unwrap());
        let launchers = self.launchers.borrow();
        let launcher = &launchers[index];
        let game_models = self.game_models.borrow();
        let icons = launcher.is_custom().then(|| self.popup_icon_shape.get());
        let games = game_models[index].get_or_init(|| {
            let scale = dock.window().scale_factor();
            let items: Vec<DockItem> =
                launcher.games.iter().map(|game| game_item(game, launcher.landscape_covers, icons, scale)).collect();
            ModelRc::new(VecModel::from(items))
        });
        popup.set_launcher_name(launcher.name.as_str().into());
        popup.set_empty_text(if launcher.is_custom() { CUSTOM_EMPTY_TEXT } else { "No installed games" }.into());
        // Set before showing: the tile shape changes the popup's size.
        popup.set_landscape(launcher.landscape_covers);
        popup.set_icons(launcher.is_custom());
        popup.set_games(games.clone());
        show_popup(&dock, &popup, center, self.vertical.get(), self.raised.get());
        self.popup_had_focus.set(false);
        self.open.set(Some(position));
        self.popup_center.set(center);
        dock.set_active_index(position as i32);
    }

    /// Moves the launcher dragged from dock position `from` to `to`, and saves the new order.
    fn move_launcher(&self, from: usize, to: usize) {
        let mut order = self.order.borrow_mut();
        if from == to || from >= order.len() || to >= order.len() {
            return;
        }
        // Its tail would point at the old position.
        self.close_popup();
        let moved = order.remove(from);
        order.insert(to, moved);
        // Rows are rewritten in place rather than removed and reinserted, so the dragged
        // tile's instance survives and still swallows the click ending the drag.
        let mut rows: Vec<DockItem> = self.items.iter().collect();
        let row = rows.remove(from);
        rows.insert(to, row);
        for (i, row) in rows.into_iter().enumerate() {
            self.items.set_row_data(i, row);
        }
        let launchers = self.launchers.borrow();
        let keys: Vec<String> = order.iter().map(|&i| launchers[i].key.clone()).collect();
        state::update(|state| state.dock.order = keys);
    }

    fn launch(&self, id: &str) {
        {
            let launchers = self.launchers.borrow();
            if let Some(game) = launchers.iter().flat_map(|l| &l.games).find(|game| game.id == id) {
                game.launch.start();
            }
        }
        self.close_popup();
    }

    /// Drops the launcher at dock `position` from the settings' `launchers` list (re-enabled
    /// from Manage Dock or by editing config.toml), or deletes it from `custom`, and takes its
    /// tile off the dock; the dock window shrinks to the remaining tiles. `launchers` keeps the
    /// entry, so `order`'s indices and the cached covers stay valid until the next `apply`.
    fn remove_launcher(&self, position: usize) {
        let Some(&index) = self.order.borrow().get(position) else { return };
        let key = self.launchers.borrow()[index].key.clone();
        let mut config = config::load();
        config.launchers.retain(|launcher| !same_launcher(launcher, &key));
        config.custom.retain(|custom| !custom.key().eq_ignore_ascii_case(&key));
        if let Err(err) = config::save(&config) {
            eprintln!("couldn't remove {key} from the dock: {err}");
            return;
        }
        // `open` is a dock position, which shifts for the tiles after this one.
        self.close_popup();
        self.order.borrow_mut().remove(position);
        self.items.remove(position);
    }

    /// Adds the launcher `key` to the settings' `launchers` list, or drops it if it's there,
    /// saves, and rebuilds the dock (a newly shown launcher goes last unless the saved order
    /// places it). A launcher that isn't installed stays off the dock even when listed.
    fn toggle_launcher(&self, key: &str) {
        let mut config = config::load();
        if config.launchers.iter().any(|entry| same_launcher(entry, key)) {
            config.launchers.retain(|entry| !same_launcher(entry, key));
        } else {
            config.launchers.push(key.to_string());
        }
        if let Err(err) = config::save(&config) {
            eprintln!("couldn't update the dock's launchers: {err}");
            return;
        }
        self.apply(&config);
    }

    /// Removes the application at `index` of the `items` of the custom launcher at dock
    /// `position` from config.toml, and rebuilds the dock and its popup without it. Skipped if
    /// config.toml was edited since the popup was filled, so the index may name another item.
    fn remove_custom_item(&self, position: usize, index: usize) {
        let Some((key, mut config)) = self.checked_custom_item(position, index) else { return };
        let Some(custom) = config.custom.iter_mut().find(|custom| custom.key().eq_ignore_ascii_case(&key)) else { return };
        custom.items.remove(index);
        if let Err(err) = config::save(&config) {
            eprintln!("couldn't remove item {index} of {key}: {err}");
            return;
        }
        self.custom_items_changed(&key, &config);
    }

    /// The key of the custom launcher at dock `position` and the settings, if its
    /// `items[index]` in config.toml is still the application the popup shows; otherwise
    /// config.toml was edited since the popup was filled, and the index may name another one.
    fn checked_custom_item(&self, position: usize, index: usize) -> Option<(String, Config)> {
        let key = self.key_at(position)?;
        let id = format!("{key}:{index}");
        let shown_path = self.launchers.borrow().iter().flat_map(|l| &l.games).find(|game| game.id == id).and_then(
            |game| match &game.launch {
                game::Launch::Open(path) => Some(path.clone()),
                _ => None,
            },
        )?;
        let config = config::load();
        let custom = config.custom.iter().find(|custom| custom.key().eq_ignore_ascii_case(&key))?;
        if !custom.items.get(index).is_some_and(|item| Path::new(item.path.trim()) == shown_path) {
            eprintln!("config.toml changed; ignoring item {index} of {key} (try Refresh Dock)");
            return None;
        }
        Some((key, config))
    }

    /// Whether the launcher at dock `position` is a custom one.
    fn custom_at(&self, position: usize) -> bool {
        self.order.borrow().get(position).is_some_and(|&index| self.launchers.borrow()[index].is_custom())
    }

    /// Applies `config`, saved with a changed application list for the custom launcher `key`
    /// (added in the "Add Application" window, or removed); that launcher's popup, if still
    /// open, reopens showing the change.
    fn custom_items_changed(&self, key: &str, config: &Config) {
        // `apply` closes the popup and rebuilds the tiles; the launcher keeps its dock position.
        let reopen = self.open.get().filter(|&position| self.key_at(position).as_deref() == Some(key));
        let center = self.popup_center.get();
        self.apply(config);
        if let Some(position) = reopen {
            if self.key_at(position).as_deref() == Some(key) {
                self.toggle_popup(position, center);
            }
        }
    }

    /// The key of the launcher at dock `position`.
    fn key_at(&self, position: usize) -> Option<String> {
        let index = *self.order.borrow().get(position)?;
        Some(self.launchers.borrow()[index].key.clone())
    }
}

/// A custom launcher's popup before anything was added to it.
const CUSTOM_EMPTY_TEXT: &str = "Right-click to add applications";

/// The built-in launchers the "Manage Dock" submenu can show or hide: key and menu label.
const BUILT_IN_LAUNCHERS: [(&str, &str); 3] = [("steam", "Steam"), ("battlenet", "Battle.net"), ("ea", "EA")];

/// Whether `entry` from the settings' `launchers` list names the launcher `key`; matches the
/// spellings `providers::scan_all` accepts.
fn same_launcher(entry: &str, key: &str) -> bool {
    let entry = entry.trim().to_ascii_lowercase();
    entry == key || (key == "battlenet" && entry == "battle.net")
}

/// The app menu (tray icon, right-click on the dock), built each time it opens so the
/// launchers' check marks follow config.toml.
fn app_menu() -> Vec<platform::MenuEntry> {
    use platform::{MenuCommand, MenuEntry};

    let enabled = config::load().launchers;
    let mut manage = vec![MenuEntry::Command(MenuCommand::AddCustomLauncher, "Custom".to_string()), MenuEntry::Separator];
    manage.extend(BUILT_IN_LAUNCHERS.iter().map(|&(key, label)| {
        let shown = enabled.iter().any(|entry| same_launcher(entry, key));
        MenuEntry::Check(MenuCommand::ToggleLauncher(key), label.to_string(), shown)
    }));
    vec![
        MenuEntry::Submenu("Manage Dock".to_string(), manage),
        MenuEntry::Command(MenuCommand::EditSettings, "Dock Settings".to_string()),
        MenuEntry::Command(MenuCommand::Refresh, "Refresh Dock".to_string()),
        MenuEntry::Separator,
        MenuEntry::Command(MenuCommand::Quit, "Quit Rusty Dock".to_string()),
    ]
}

/// Where the dock may sit with its top-left corner at (`x`, `y`): within the area covered by all
/// monitors (which can include corners no monitor shows), so it can't be dragged out of reach.
fn keep_on_screen(x: i32, y: i32, size: slint::PhysicalSize) -> (i32, i32) {
    let (width, height) = (size.width as i32, size.height as i32);
    match platform::virtual_screen() {
        Some((left, top, right, bottom)) => (x.min(right - width).max(left), y.min(bottom - height).max(top)),
        None => (x, y),
    }
}

/// A tile or row count from the settings, at least 1 and small enough for Slint's `int`.
fn count(n: u32) -> i32 {
    n.clamp(1, 100) as i32
}

/// A background color from the settings.
fn color(hex: &str, opacity: f64) -> slint::Color {
    let [r, g, b, a] = config::rgba(hex, opacity);
    slint::Color::from_argb_u8(a, r, g, b)
}

/// Shows the popup centered on a dock icon, on the side of the dock facing the larger part of
/// its monitor: above or below a horizontal dock, left or right of a `vertical` one. `center` is
/// the icon's center along the dock, in logical pixels. While the dock is `raised` the popup is
/// too, or it could open behind the window the dock rose above.
fn show_popup(dock: &Dock, popup: &GamesPopup, center: f32, vertical: bool, raised: bool) {
    let scale = dock.window().scale_factor();
    let dock_pos = dock.window().position();
    let dock_size = dock.window().size();
    let (dock_width, dock_height) = (dock_size.width as i32, dock_size.height as i32);
    let monitor = platform::monitor_bounds(dock.window());
    let side = match (vertical, monitor) {
        (false, Some((_, top, _, bottom))) if dock_pos.y + dock_height / 2 < (top + bottom) / 2 => PopupSide::Below,
        (false, _) => PopupSide::Above,
        (true, Some((left, _, right, _))) if dock_pos.x + dock_width / 2 < (left + right) / 2 => PopupSide::Right,
        (true, _) => PopupSide::Left,
    };
    // Set first: the tail's edge changes the popup's size.
    popup.set_side(side);
    let width = (popup.get_panel_width() * scale) as i32;
    let height = (popup.get_panel_height() * scale) as i32;
    let gap = (POPUP_GAP * scale) as i32;
    let margin = (POPUP_EDGE_MARGIN * scale) as i32;
    // Centered on the launcher along the dock, then shifted back onto the dock's monitor near
    // its edge; the tail stays on the launcher however far the bubble moved.
    let (x, y, tail) = if vertical {
        let anchor = dock_pos.y + (center * scale) as i32;
        let mut y = anchor - height / 2;
        if let Some((_, top, _, bottom)) = monitor {
            y = y.min(bottom - margin - height).max(top + margin);
        }
        let x = if matches!(side, PopupSide::Right) { dock_pos.x + dock_width + gap } else { dock_pos.x - width - gap };
        (x, y, anchor - y)
    } else {
        let anchor = dock_pos.x + (center * scale) as i32;
        let mut x = anchor - width / 2;
        if let Some((left, _, right, _)) = monitor {
            x = x.min(right - margin - width).max(left + margin);
        }
        let y = if matches!(side, PopupSide::Below) { dock_pos.y + dock_height + gap } else { dock_pos.y - height - gap };
        (x, y, anchor - x)
    };
    popup.set_tail_pos(tail as f32 / scale);
    popup.show().unwrap();
    popup.window().set_position(PhysicalPosition::new(x, y));
    when_created(popup.as_weak(), move |popup| {
        platform::hide_from_taskbar(popup.window());
        platform::set_topmost(popup.window(), raised);
        // Last, since `hide_from_taskbar` may hide and reshow the window.
        platform::bring_to_foreground(popup.window());
    });
}

/// How often `when_created` checks for the native window, and for how many tries.
const CREATE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CREATE_POLL_TRIES: u32 = 200;

/// Runs `f` once the shown component's native window exists (winit creates it only once the
/// event loop runs), so platform calls on it take effect. Gives up after `CREATE_POLL_TRIES`.
fn when_created<C: slint::ComponentHandle + 'static>(component: slint::Weak<C>, f: impl FnOnce(C) + 'static) {
    poll_created(component, f, CREATE_POLL_TRIES);
}

fn poll_created<C: slint::ComponentHandle + 'static>(
    component: slint::Weak<C>,
    f: impl FnOnce(C) + 'static,
    tries: u32,
) {
    let Some(strong) = component.upgrade() else { return };
    if platform::has_native_window(strong.window()) {
        f(strong);
    } else if tries == 0 {
        eprintln!("native window never appeared; skipping its setup");
    } else {
        Timer::single_shot(CREATE_POLL_INTERVAL, move || poll_created(component, f, tries - 1));
    }
}

/// Corner radius of a launcher tile in logical pixels, matching the `Dock`'s `border-radius` in
/// main.slint; keep in sync. Icons fill the tile edge to edge, so they are cut to this shape
/// here: the software renderer's `clip` ignores `border-radius`.
fn icon_radius(shape: config::IconShape, icon_size: u32) -> f32 {
    match shape {
        config::IconShape::Circle => icon_size as f32 / 2.0,
        config::IconShape::Curve => icon_size as f32 / 4.0,
        config::IconShape::Flat => 0.0,
    }
}

/// The dock's tiles, `launchers` in dock `order`; `icon_size` is a tile's side in logical
/// pixels, `radius` its corner radius in physical pixels.
fn launcher_rows(launchers: &[Launcher], order: &[usize], icon_size: u32, radius: f32, scale: f32) -> Vec<DockItem> {
    order.iter().map(|&i| launcher_item(&launchers[i], icon_size, radius, scale)).collect()
}

fn launcher_item(launcher: &Launcher, icon_size: u32, radius: f32, scale: f32) -> DockItem {
    let (width, height) = physical_size((icon_size, icon_size), scale);
    let icon = launcher.icon.as_deref().and_then(|path| Some(fit_image(icon_image(path)?, width, height, radius)));
    dock_item("", &launcher.name, icon)
}

/// A program's own icon (`.exe`) or an image file, fitted to `width` x `height` physical
/// pixels with square corners (the custom launcher windows' previews).
fn load_icon(path: &Path, width: u32, height: u32) -> Option<Image> {
    Some(fit_image(icon_image(path)?, width, height, 0.0))
}


fn is_exe(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// A program's own icon (`.exe`) or an image file, decoded.
fn icon_image(path: &Path) -> Option<image::DynamicImage> {
    if is_exe(path) {
        let (w, h, rgba) = platform::exe_icon(path)?;
        Some(image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, rgba)?))
    } else {
        read_image(path)
    }
}

/// Larger files aren't artwork (a custom item can be any file), so they aren't read whole.
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

/// An image file, decoded by content, so one with no or an unusual extension (Battle.net's
/// cached artwork) still loads.
fn read_image(path: &Path) -> Option<image::DynamicImage> {
    if std::fs::metadata(path).ok()?.len() > MAX_IMAGE_BYTES {
        return None;
    }
    image::load_from_memory(&std::fs::read(path).ok()?).ok()
}

/// A game's tile; with `icons` (custom launchers' applications) it is a square icon, a
/// program's or an image, drawn like a launcher on the dock rather than as cover art, and cut
/// to the popup's icon shape.
fn game_item(game: &Game, landscape: bool, icons: Option<config::IconShape>, scale: f32) -> DockItem {
    let tile = match icons {
        Some(_) => ICON_TILE,
        None if landscape => LANDSCAPE_TILE,
        None => PORTRAIT_TILE,
    };
    let (width, height) = physical_size(tile, scale);
    let cover = game.cover.as_deref().and_then(|path| match icons {
        Some(shape) => Some(fit_image(icon_image(path)?, width, height, icon_radius(shape, ICON_TILE.0) * scale)),
        None => load_cover(path, width, height, GAME_TILE_RADIUS * scale),
    });
    dock_item(&game.id, &game.title, cover)
}

fn slint_shape(shape: config::IconShape) -> IconShape {
    match shape {
        config::IconShape::Circle => IconShape::Circle,
        config::IconShape::Curve => IconShape::Curve,
        config::IconShape::Flat => IconShape::Flat,
    }
}

/// A tile's size in physical pixels at the window's scale factor.
fn physical_size((width, height): (u32, u32), scale: f32) -> (u32, u32) {
    ((width as f32 * scale).ceil() as u32, (height as f32 * scale).ceil() as u32)
}

fn load_cover(path: &Path, width: u32, height: u32, radius: f32) -> Option<Image> {
    Some(fit_image(read_image(path)?, width, height, radius))
}

/// Scales and center-crops an image to exactly `width` x `height` physical pixels, keeping its
/// aspect ratio, with corners rounded to `radius`. Slint's software renderer samples crudely
/// when scaling and clips children to their parent's rectangle, not its rounded corners, so
/// the image must arrive at the size and shape it is drawn; it also keeps source art, up to
/// 85x the pixels a tile shows, out of memory.
fn fit_image(image: image::DynamicImage, width: u32, height: u32, radius: f32) -> Image {
    let mut rgba = image.resize_to_fill(width, height, image::imageops::FilterType::CatmullRom).to_rgba8();
    if radius > 0.0 {
        round_corners(&mut rgba, radius);
    }
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

/// Makes the pixels outside `radius` arcs at the corners transparent, anti-aliased over one pixel.
fn round_corners(image: &mut image::RgbaImage, radius: f32) {
    let (width, height) = (image.width() as f32, image.height() as f32);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        // How far the pixel's center lies beyond the nearest arc's center, along each axis.
        let dx = f32::max(radius - px, px - (width - radius)).max(0.0);
        let dy = f32::max(radius - py, py - (height - radius)).max(0.0);
        if dx == 0.0 || dy == 0.0 {
            continue;
        }
        let coverage = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        pixel[3] = (pixel[3] as f32 * coverage).round() as u8;
    }
}

fn dock_item(id: &str, title: &str, icon: Option<Image>) -> DockItem {
    DockItem {
        id: id.into(),
        title: title.into(),
        has_icon: icon.is_some(),
        icon: icon.unwrap_or_default(),
        label: initials(title).into(),
    }
}

/// Up to two letters standing in for a missing icon, e.g. "Counter-Strike 2" -> "C2".
fn initials(title: &str) -> String {
    title
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Where the dock starts before it has been dragged: centered at the bottom of the primary
/// monitor's work area (the screen minus the taskbar), or at its left edge when `vertical`.
#[cfg(windows)]
fn place_default(window: &slint::Window, vertical: bool) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

    let mut area = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut area as *mut RECT as *mut _, 0) };
    if ok == 0 {
        return;
    }

    let size = window.size();
    let (width, height) = (size.width as i32, size.height as i32);
    let (x, y) = if vertical {
        (area.left, area.top + (area.bottom - area.top - height) / 2)
    } else {
        (area.left + (area.right - area.left - width) / 2, area.bottom - height)
    };
    window.set_position(slint::PhysicalPosition::new(x, y));
}

#[cfg(not(windows))]
fn place_default(_window: &slint::Window, _vertical: bool) {}
