//! Small OS-specific helpers.

use std::path::Path;

/// Pixel size requested for executable icons; Windows scales if the exe lacks this size.
#[cfg(windows)]
const ICON_SIZE: u32 = 128;

/// Opens a URI (e.g. `steam://...`) with the handler the OS has registered for it.
#[cfg(windows)]
pub fn open_uri(uri: &str) {
    if !shell_open(uri, None, None) {
        eprintln!("failed to open {uri}");
    }
}

/// Opens a program, shortcut or other file as a double-click in Explorer would, with its own
/// folder as the working directory (games often load files relative to it).
#[cfg(windows)]
pub fn open_file(path: &Path) {
    let Some(file) = path.to_str() else { return };
    if !shell_open(file, None, path.parent().and_then(Path::to_str)) {
        eprintln!("failed to open {file}");
    }
}

#[cfg(not(windows))]
pub fn open_file(path: &Path) {
    if let Some(file) = path.to_str() {
        open_uri(file);
    }
}

/// Opens a text file in the app registered for its type, or in Notepad when there is none
/// (`.toml` often has no association).
#[cfg(windows)]
pub fn edit_file(path: &Path) {
    let Some(file) = path.to_str() else { return };
    if !shell_open(file, None, None) && !shell_open("notepad.exe", Some(&format!("\"{file}\"")), None) {
        eprintln!("failed to open {file}");
    }
}

#[cfg(not(windows))]
pub fn edit_file(path: &Path) {
    if let Some(file) = path.to_str() {
        open_uri(file);
    }
}

/// ShellExecuteW's "open" verb, in `dir` if given; whether it succeeded.
#[cfg(windows)]
fn shell_open(file: &str, params: Option<&str>, dir: Option<&str>) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = wide("open");
    let file = wide(file);
    let params = params.map(wide);
    let dir = dir.map(wide);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            params.as_ref().map_or(std::ptr::null(), |params| params.as_ptr()),
            dir.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a value greater than 32 on success.
    result as usize > 32
}

#[cfg(not(windows))]
pub fn open_uri(uri: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Err(err) = std::process::Command::new(opener).arg(uri).spawn() {
        eprintln!("failed to open {uri}: {err}");
    }
}

/// The per-user directory for app settings: `%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` (or `~/.config`) elsewhere.
pub fn config_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    let non_empty = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        non_empty("APPDATA")
    } else if cfg!(target_os = "macos") {
        Some(non_empty("HOME")?.join("Library/Application Support"))
    } else {
        non_empty("XDG_CONFIG_HOME").or_else(|| Some(non_empty("HOME")?.join(".config")))
    }
}

/// Makes a shown window a tool window: no taskbar button and not in Alt+Tab, like a dock
/// or tooltip. Call it once the native window exists (`main::when_created`).
/// Setting the style once isn't enough: winit rewrites the whole extended style (with
/// `WS_EX_APPWINDOW`) whenever Slint changes a window flag, so a subclass keeps it a tool window.
#[cfg(windows)]
pub fn hide_from_taskbar(window: &slint::Window) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SW_HIDE, SW_SHOW, SetWindowLongPtrW, ShowWindow,
    };

    let Some(hwnd) = hwnd(window) else { return };
    unsafe {
        // Installing it again for the same window just replaces the old one.
        SetWindowSubclass(hwnd, Some(tool_window_proc), TOOL_WINDOW_SUBCLASS, 0);
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let tool_style = tool_ex_style(style as u32) as isize;
        if tool_style == style {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool_style);
        // The taskbar only drops the button when the window is shown again.
        ShowWindow(hwnd, SW_HIDE);
        ShowWindow(hwnd, SW_SHOW);
    }
}

#[cfg(windows)]
const TOOL_WINDOW_SUBCLASS: usize = 1;

#[cfg(windows)]
fn tool_ex_style(style: u32) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_EX_APPWINDOW, WS_EX_TOOLWINDOW};
    (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
}

/// Turns every extended style about to be set on the window (winit's included) into a tool
/// window's.
#[cfg(windows)]
unsafe extern "system" fn tool_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, STYLESTRUCT, WM_STYLECHANGING};

    if msg == WM_STYLECHANGING && wparam as i32 == GWL_EXSTYLE && lparam != 0 {
        let change = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
        change.styleNew = tool_ex_style(change.styleNew);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(not(windows))]
pub fn hide_from_taskbar(_window: &slint::Window) {}

/// Held while this is the session's only rusty_dock, so a second launch (e.g. autostart plus
/// a manual start) doesn't add a second dock. Released on drop.
#[cfg(windows)]
pub struct InstanceLock(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl InstanceLock {
    /// `None` if another rusty_dock already runs in this session.
    pub fn acquire() -> Option<InstanceLock> {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name = wide("Local\\rusty_dock");
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        // A mutex that can't be created can't tell either way, so run.
        if !handle.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(InstanceLock(handle))
    }
}

#[cfg(windows)]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}

#[cfg(not(windows))]
pub struct InstanceLock;

#[cfg(not(windows))]
impl InstanceLock {
    pub fn acquire() -> Option<InstanceLock> {
        Some(InstanceLock)
    }
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE: &str = "Rusty Dock";
/// The value's earlier name, removed so the dock isn't started twice.
#[cfg(windows)]
const OLD_RUN_VALUE: &str = "rusty_dock";

/// Adds this executable to (or removes it from) the programs Windows starts at sign-in, under
/// `HKCU\...\Run`, so no admin rights are needed. Written only when it changes; rewritten when
/// the exe has moved.
#[cfg(windows)]
pub fn set_autostart(enabled: bool) {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};

    let key = wide(RUN_KEY);
    if read_hkcu_string(RUN_KEY, OLD_RUN_VALUE).is_some() {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), wide(OLD_RUN_VALUE).as_ptr()) };
    }
    let current = read_hkcu_string(RUN_KEY, RUN_VALUE);
    let value = wide(RUN_VALUE);
    if !enabled {
        if current.is_some() {
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        }
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let command = format!("\"{}\"", exe.display());
    if current.as_deref() == Some(command.as_str()) {
        return;
    }
    let data = wide(&command);
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if err != 0 {
        eprintln!("failed to set autostart: error {err}");
    }
}

#[cfg(not(windows))]
pub fn set_autostart(_enabled: bool) {}

/// Commands in the app menu, opened from the tray icon or by right-clicking the dock, and in
/// a launcher tile's menu.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum MenuCommand {
    /// Adds a user-defined launcher (any program) to the dock.
    AddCustomLauncher,
    /// Opens the settings window.
    EditSettings,
    /// Rereads the settings and rescans launchers and games, e.g. after installing a game.
    Refresh,
    Quit,
    /// Takes the launcher at this dock position off the dock.
    RemoveLauncher(usize),
    /// Opens the custom launcher at this dock position for editing.
    EditCustomLauncher(usize),
    /// Picks a file to add to the popup of the custom launcher at this dock position.
    AddCustomItem(usize),
    /// Opens the application at this index of its `items`, in the popup of the custom launcher
    /// at this dock position, for editing.
    EditCustomItem(usize, usize),
    /// Removes the application at this index of its `items` from the popup of the custom
    /// launcher at this dock position.
    RemoveCustomItem(usize, usize),
    /// Puts the launcher with this key ("steam", ...) on the dock, or takes it off.
    ToggleLauncher(&'static str),
}

/// One entry of a native menu.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum MenuEntry {
    Command(MenuCommand, String),
    /// A command shown with a check mark when the flag is set.
    Check(MenuCommand, String, bool),
    Submenu(String, Vec<MenuEntry>),
    Separator,
}

/// Sent by the shell for clicks on the tray icon.
#[cfg(windows)]
const WM_TRAY: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 1;
/// Posted by `show_app_menu`.
#[cfg(windows)]
const WM_SHOW_MENU: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 2;
/// Broadcast when Explorer (re)starts and tray icons must be added again; 0 until registered.
#[cfg(windows)]
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(windows)]
struct TrayState {
    hwnd: windows_sys::Win32::Foundation::HWND,
    icon: windows_sys::Win32::UI::Shell::NOTIFYICONDATAW,
    on_command: Box<dyn Fn(MenuCommand)>,
    /// Builds the app menu each time it opens, so check marks follow the current settings.
    app_menu: Box<dyn Fn() -> Vec<MenuEntry>>,
    /// The entries `WM_SHOW_MENU` opens, set by `show_app_menu` / `show_launcher_menu`.
    pending_menu: Vec<MenuEntry>,
}

#[cfg(windows)]
thread_local! {
    static TRAY: std::cell::RefCell<Option<TrayState>> = std::cell::RefCell::new(None);
}

/// The notification-area icon and the hidden window that owns it and the app menu. Removed
/// on drop.
#[cfg(windows)]
pub struct Tray(());

#[cfg(windows)]
impl Tray {
    /// Adds the tray icon (the exe's own icon); its menu is the one `app_menu` builds, and
    /// `on_command` runs for each command picked from it. Create it on the UI thread: Slint's
    /// event loop then delivers the hidden window's messages too.
    pub fn new(
        tooltip: &str,
        app_menu: impl Fn() -> Vec<MenuEntry> + 'static,
        on_command: impl Fn(MenuCommand) + 'static,
    ) -> Option<Tray> {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFYICONDATAW, Shell_NotifyIconW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, GetSystemMetrics, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, WNDCLASSW,
        };

        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let class_name = wide("rusty_dock_tray");
        let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
        class.lpfnWndProc = Some(tray_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        if unsafe { RegisterClassW(&class) } == 0 {
            return None;
        }
        // A never-shown top-level window rather than a message-only one: only top-level
        // windows receive the "TaskbarCreated" broadcast.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                class_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        let taskbar_created = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
        TASKBAR_CREATED.store(taskbar_created, std::sync::atomic::Ordering::Relaxed);

        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of_val(&data) as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = app_icon(instance, size);
        let max_tip = data.szTip.len() - 1;
        for (dst, src) in data.szTip.iter_mut().zip(tooltip.encode_utf16().take(max_tip)) {
            *dst = src;
        }
        // Fails while Explorer isn't running; added again on "TaskbarCreated".
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        TRAY.with(|tray| {
            *tray.borrow_mut() = Some(TrayState {
                hwnd,
                icon: data,
                on_command: Box::new(on_command),
                app_menu: Box::new(app_menu),
                pending_menu: Vec::new(),
            })
        });
        Some(Tray(()))
    }
}

#[cfg(windows)]
impl Drop for Tray {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::Shell::{NIM_DELETE, Shell_NotifyIconW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, DestroyWindow};

        let Some(state) = TRAY.with(|tray| tray.borrow_mut().take()) else { return };
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &state.icon);
            DestroyIcon(state.icon.hIcon);
            DestroyWindow(state.hwnd);
        }
    }
}

#[cfg(not(windows))]
pub struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub fn new(
        _tooltip: &str,
        _app_menu: impl Fn() -> Vec<MenuEntry> + 'static,
        _on_command: impl Fn(MenuCommand) + 'static,
    ) -> Option<Tray> {
        None
    }
}

/// Opens the app menu at the mouse cursor. Posted to the tray's window so the menu's modal
/// loop runs from that window's message handler, not inside a Slint callback.
#[cfg(windows)]
pub fn show_app_menu() {
    show_menu(app_menu());
}

#[cfg(not(windows))]
pub fn show_app_menu() {}

/// Opens a launcher tile's menu at the mouse cursor, like `show_app_menu`; `position` is the
/// tile's dock position, `name` the launcher's display name. A `custom` launcher can also be
/// edited.
#[cfg(windows)]
pub fn show_launcher_menu(position: usize, name: &str, custom: bool) {
    let mut entries = Vec::new();
    if custom {
        entries.push(MenuEntry::Command(MenuCommand::EditCustomLauncher(position), format!("Edit {name}")));
        entries.push(MenuEntry::Separator);
    }
    entries.push(MenuEntry::Command(MenuCommand::RemoveLauncher(position), format!("Remove {name} from Dock")));
    show_menu(entries);
}

#[cfg(not(windows))]
pub fn show_launcher_menu(_position: usize, _name: &str, _custom: bool) {}

/// Opens the menu of the open popup of the custom launcher at dock `position`, like
/// `show_app_menu`; right-clicked on an application, `item` is its index in the launcher's
/// `items` and its name, which add "Edit" and "Remove" entries.
#[cfg(windows)]
pub fn show_custom_popup_menu(position: usize, item: Option<(usize, &str)>) {
    let mut entries = vec![MenuEntry::Command(MenuCommand::AddCustomItem(position), "Add application".to_string())];
    if let Some((index, name)) = item {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Command(MenuCommand::EditCustomItem(position, index), format!("Edit {name}")));
        entries.push(MenuEntry::Command(MenuCommand::RemoveCustomItem(position, index), format!("Remove {name}")));
    }
    show_menu(entries);
}

#[cfg(not(windows))]
pub fn show_custom_popup_menu(_position: usize, _item: Option<(usize, &str)>) {}

#[cfg(windows)]
fn show_menu(entries: Vec<MenuEntry>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

    let hwnd = TRAY.with(|tray| {
        tray.borrow_mut().as_mut().map(|state| {
            state.pending_menu = entries;
            state.hwnd
        })
    });
    if let Some(hwnd) = hwnd {
        unsafe { PostMessageW(hwnd, WM_SHOW_MENU, 0, 0) };
    }
}

/// The app menu's entries, from the builder given to `Tray::new`.
#[cfg(windows)]
fn app_menu() -> Vec<MenuEntry> {
    TRAY.with(|tray| tray.borrow().as_ref().map(|state| (state.app_menu)())).unwrap_or_default()
}

#[cfg(windows)]
unsafe extern "system" fn tray_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::{NIM_ADD, Shell_NotifyIconW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_LBUTTONUP, WM_RBUTTONUP};

    // Without NOTIFYICON_VERSION_4, the tray message's lParam is the mouse message itself.
    if msg == WM_SHOW_MENU {
        // Taken out first: the menu's modal loop dispatches messages, which may reenter here.
        let entries = TRAY.with(|tray| tray.borrow_mut().as_mut().map(|state| std::mem::take(&mut state.pending_menu)));
        let entries = entries.filter(|entries| !entries.is_empty()).unwrap_or_else(app_menu);
        if entries.is_empty() {
            return 0;
        }
        track_menu(hwnd, &entries);
        return 0;
    }
    if msg == WM_TRAY && matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
        track_menu(hwnd, &app_menu());
        return 0;
    }
    let taskbar_created = TASKBAR_CREATED.load(std::sync::atomic::Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        TRAY.with(|tray| {
            if let Some(state) = &*tray.borrow() {
                unsafe { Shell_NotifyIconW(NIM_ADD, &state.icon) };
            }
        });
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Fills `menu` with `entries`, recursing into submenus. Each command's id is its index in
/// `commands` + 1, since 0 means none was picked.
#[cfg(windows)]
fn append_entries(
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    entries: &[MenuEntry],
    commands: &mut Vec<MenuCommand>,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    };

    for entry in entries {
        match entry {
            MenuEntry::Command(command, label) | MenuEntry::Check(command, label, false) => {
                commands.push(*command);
                let label = wide(label);
                unsafe { AppendMenuW(menu, MF_STRING, commands.len(), label.as_ptr()) };
            }
            MenuEntry::Check(command, label, true) => {
                commands.push(*command);
                let label = wide(label);
                unsafe { AppendMenuW(menu, MF_STRING | MF_CHECKED, commands.len(), label.as_ptr()) };
            }
            MenuEntry::Separator => {
                unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null()) };
            }
            MenuEntry::Submenu(label, children) => {
                let submenu = unsafe { CreatePopupMenu() };
                if submenu.is_null() {
                    continue;
                }
                append_entries(submenu, children, commands);
                // Attached submenus are destroyed along with their parent menu.
                let label = wide(label);
                unsafe { AppendMenuW(menu, MF_STRING | MF_POPUP, submenu as usize, label.as_ptr()) };
            }
        }
    }
}

/// Shows a menu of `entries` at the cursor and, once one is picked, runs its command from
/// Slint's event loop.
#[cfg(windows)]
fn track_menu(hwnd: windows_sys::Win32::Foundation::HWND, entries: &[MenuEntry]) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
    };

    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    let mut commands = Vec::new();
    append_entries(menu, entries, &mut commands);
    let mut cursor = POINT { x: 0, y: 0 };
    let chosen = unsafe {
        GetCursorPos(&mut cursor);
        // Documented TrackPopupMenu quirk: without a foreground owner the menu doesn't close
        // when clicking elsewhere, and without the WM_NULL it can't reopen right away.
        SetForegroundWindow(hwnd);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
        let chosen = TrackPopupMenu(menu, flags, cursor.x, cursor.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        chosen
    };
    let Some(&command) = (chosen as usize).checked_sub(1).and_then(|index| commands.get(index)) else { return };
    let _ = slint::invoke_from_event_loop(move || {
        TRAY.with(|tray| {
            if let Some(state) = &*tray.borrow() {
                (state.on_command)(command);
            }
        });
    });
}

/// The exe's icon (resource 1, embedded by build.rs) at `size` pixels square, picked from the
/// .ico's sizes by Windows, or the stock application icon if it's missing.
#[cfg(windows)]
fn app_icon(
    instance: windows_sys::Win32::Foundation::HMODULE,
    size: i32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW, LoadImageW};

    // MAKEINTRESOURCEW(1).
    let icon = unsafe { LoadImageW(instance, 1usize as windows_sys::core::PCWSTR, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) };
    if icon.is_null() { unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) } } else { icon }
}

/// Keeps a window above all normal windows (`true`), or returns it to the normal stacking
/// order (`false`), without activating it.
#[cfg(windows)]
pub fn set_topmost(window: &slint::Window, topmost: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    let Some(hwnd) = hwnd(window) else { return };
    let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}

#[cfg(not(windows))]
pub fn set_topmost(_window: &slint::Window, _topmost: bool) {}

/// Window classes of the desktop, which Explorer keeps at the very bottom.
#[cfg(windows)]
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

/// Ends a window's topmost state and puts it just behind the foreground window, without
/// activating it: `HWND_NOTOPMOST` alone leaves it above every normal window, the foreground one
/// included. It stays above normal windows when the foreground is topmost (going behind it would
/// keep this window topmost) or the desktop (going behind it would hide this window).
#[cfg(windows)]
pub fn drop_behind_foreground(window: &slint::Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SetWindowPos, WS_EX_TOPMOST,
    };

    set_topmost(window, false);
    let Some(hwnd) = hwnd(window) else { return };
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() || foreground == hwnd {
        return;
    }
    if unsafe { GetWindowLongPtrW(foreground, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0 {
        return;
    }
    let mut buf = [0u16; 32];
    let len = unsafe { GetClassNameW(foreground, buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
    if DESKTOP_CLASSES.contains(&class.as_str()) {
        return;
    }
    unsafe { SetWindowPos(hwnd, foreground, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}

#[cfg(not(windows))]
pub fn drop_behind_foreground(_window: &slint::Window) {}

// /// Puts a window behind all others, without activating it. It stays above the desktop, which
// /// Explorer keeps at the very bottom, so it still shows when nothing covers it.
// #[cfg(windows)]
// pub fn send_to_back(window: &slint::Window) {
//     use windows_sys::Win32::UI::WindowsAndMessaging::{
//         HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
//     };
//
//     let Some(hwnd) = hwnd(window) else { return };
//     unsafe { SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
// }
//
// #[cfg(not(windows))]
// pub fn send_to_back(_window: &slint::Window) {}

/// Asks Windows to make a window the foreground one. A window opened by a click normally gets
/// that anyway, but not always while the Start menu is closing.
#[cfg(windows)]
pub fn bring_to_foreground(window: &slint::Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

    let Some(hwnd) = hwnd(window) else { return };
    unsafe { SetForegroundWindow(hwnd) };
}

#[cfg(not(windows))]
pub fn bring_to_foreground(_window: &slint::Window) {}

/// The bounding box of all monitors as (left, top, right, bottom) in physical pixels, to keep a
/// restored window position on screen after a monitor is unplugged.
#[cfg(windows)]
pub fn virtual_screen() -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    (width > 0 && height > 0).then_some((left, top, left + width, top + height))
}

#[cfg(not(windows))]
pub fn virtual_screen() -> Option<(i32, i32, i32, i32)> {
    None
}

/// The monitor a shown window is on (or nearest to), as (left, top, right, bottom) in physical
/// pixels, so the popup can be kept from spilling off its edge.
#[cfg(windows)]
pub fn monitor_bounds(window: &slint::Window) -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};

    let monitor = unsafe { MonitorFromWindow(hwnd(window)?, MONITOR_DEFAULTTONEAREST) };
    let empty = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: empty, rcWork: empty, dwFlags: 0 };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let rc = info.rcMonitor;
    Some((rc.left, rc.top, rc.right, rc.bottom))
}

#[cfg(not(windows))]
pub fn monitor_bounds(_window: &slint::Window) -> Option<(i32, i32, i32, i32)> {
    None
}

/// Whether Slint has created the native window yet. With winit it only does so once the event
/// loop runs, so right after `show()` the window calls in this module would do nothing.
#[cfg(windows)]
pub fn has_native_window(window: &slint::Window) -> bool {
    hwnd(window).is_some()
}

#[cfg(not(windows))]
pub fn has_native_window(_window: &slint::Window) -> bool {
    true
}

/// The native window behind a shown Slint window.
#[cfg(windows)]
fn hwnd(window: &slint::Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let slint_handle = window.window_handle();
    let handle = slint_handle.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return None };
    Some(win32.hwnd.get() as windows_sys::Win32::Foundation::HWND)
}

/// Executables of the shell menus the dock rises with, like the taskbar does.
#[cfg(windows)]
const SHELL_MENUS: [&str; 2] = ["StartMenuExperienceHost.exe", "SearchHost.exe"];

/// Whether the Start menu (or Search) is in front, e.g. after pressing the Win key.
#[cfg(windows)]
pub fn start_menu_in_foreground() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return false;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(process) };
    if ok == 0 {
        return false;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let exe = path.rsplit('\\').next().unwrap_or("");
    SHELL_MENUS.iter().any(|menu| exe.eq_ignore_ascii_case(menu))
}

#[cfg(not(windows))]
pub fn start_menu_in_foreground() -> bool {
    false
}

/// Whether another app's window is in front, i.e. the user clicked or switched away from us.
#[cfg(windows)]
pub fn other_app_in_foreground() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let window = unsafe { GetForegroundWindow() };
    // Briefly null while focus is changing hands.
    !window.is_null() && !is_own_window(window)
}

#[cfg(not(windows))]
pub fn other_app_in_foreground() -> bool {
    false
}

/// Whether one of our windows is in front.
#[cfg(windows)]
pub fn app_in_foreground() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    is_own_window(unsafe { GetForegroundWindow() })
}

#[cfg(not(windows))]
pub fn app_in_foreground() -> bool {
    false
}

// /// Whether the mouse is on one of our visible windows (the dock, the popup, a menu). A window
// /// covering ours counts as that window's, unlike a test against our windows' rectangles.
// #[cfg(windows)]
// pub fn cursor_over_app() -> bool {
//     use windows_sys::Win32::Foundation::POINT;
//     use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint};
//
//     let mut cursor = POINT { x: 0, y: 0 };
//     if unsafe { GetCursorPos(&mut cursor) } == 0 {
//         return false;
//     }
//     is_own_window(unsafe { WindowFromPoint(cursor) })
// }
//
// /// Always true elsewhere, so without a way to tell, nothing acts on the mouse having left.
// #[cfg(not(windows))]
// pub fn cursor_over_app() -> bool {
//     true
// }

#[cfg(windows)]
fn is_own_window(window: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid == std::process::id()
}

/// Hands the process's resident pages back to Windows. Startup (scanning launchers, decoding
/// art) and the popup touch a few MB that an idle dock never uses again; only the pages it
/// touches afterwards come back.
#[cfg(windows)]
pub fn trim_working_set() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

    // Passing usize::MAX for both sizes means "remove as many pages as possible".
    unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
}

#[cfg(not(windows))]
pub fn trim_working_set() {}

/// Reads a string value from HKEY_CURRENT_USER.
#[cfg(windows)]
pub fn read_hkcu_string(subkey: &str, value: &str) -> Option<String> {
    read_reg_string(windows_sys::Win32::System::Registry::HKEY_CURRENT_USER, subkey, value)
}

/// Reads a string value from HKEY_LOCAL_MACHINE.
#[cfg(windows)]
pub fn read_hklm_string(subkey: &str, value: &str) -> Option<String> {
    read_reg_string(windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE, subkey, value)
}

#[cfg(not(windows))]
pub fn read_hklm_string(_subkey: &str, _value: &str) -> Option<String> {
    None
}

/// Names of a HKEY_LOCAL_MACHINE key's subkeys; empty if the key can't be opened.
#[cfg(windows)]
pub fn hklm_subkeys(subkey: &str) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    };

    let path = wide(subkey);
    let mut key: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) } != 0 {
        return Vec::new();
    }
    let mut names = Vec::new();
    // Key names are at most 255 characters.
    let mut buf = [0u16; 256];
    for index in 0u32.. {
        let mut len = buf.len() as u32;
        let err = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // ERROR_NO_MORE_ITEMS once every subkey has been listed.
        if err != 0 {
            break;
        }
        names.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    unsafe { RegCloseKey(key) };
    names
}

#[cfg(not(windows))]
pub fn hklm_subkeys(_subkey: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
fn read_reg_string(root: windows_sys::Win32::System::Registry::HKEY, subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};

    let subkey = wide(subkey);
    let value = wide(value);
    let mut buf = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let err = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if err != 0 {
        return None;
    }
    // `size` is in bytes and includes the terminating NUL.
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Asks the user for an existing file with the standard Open dialog; `None` when cancelled.
/// `filter` is label/pattern pairs, each NUL-terminated ("Images\0*.png;*.jpg\0"). Blocks
/// until the dialog closes, so call it off the UI thread: its modal loop would otherwise run
/// inside a Slint callback. The dialog has no owner window for the same reason.
#[cfg(windows)]
pub fn pick_file(title: &str, filter: &str) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    // A second NUL ends the filter list.
    let filter: Vec<u16> = filter.encode_utf16().chain([0, 0]).collect();
    let title = wide(title);
    let mut file = vec![0u16; 1024];
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.nFilterIndex = 1;
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;

    // The dialog's shell parts (folder view, places) need COM on this thread.
    let com = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as _) };
    let ok = unsafe { GetOpenFileNameW(&mut dialog) };
    if com >= 0 {
        unsafe { CoUninitialize() };
    }
    if ok == 0 {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

#[cfg(not(windows))]
pub fn pick_file(_title: &str, _filter: &str) -> Option<std::path::PathBuf> {
    None
}

/// An executable's icon as RGBA pixels: (width, height, pixels).
#[cfg(windows)]
pub fn exe_icon(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::DeleteObject;
    use windows_sys::Win32::UI::Shell::SHDefExtractIconW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let file = wide(path.to_str()?);
    let mut icon: HICON = std::ptr::null_mut();
    let hr = unsafe { SHDefExtractIconW(file.as_ptr(), 0, 0, &mut icon, std::ptr::null_mut(), ICON_SIZE) };
    if hr != 0 || icon.is_null() {
        return None;
    }

    // GetIconInfo hands us copies of the icon's bitmaps, which we must delete.
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetIconInfo(icon, &mut info) };
    unsafe { DestroyIcon(icon) };
    if ok == 0 {
        return None;
    }
    let pixels = bitmap_rgba(info.hbmColor);
    unsafe {
        DeleteObject(info.hbmColor);
        DeleteObject(info.hbmMask);
    }
    pixels
}

#[cfg(not(windows))]
pub fn exe_icon(_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(windows)]
fn bitmap_rgba(bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW, ReleaseDC,
    };

    if bitmap.is_null() {
        return None;
    }
    let mut bmp: BITMAP = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<BITMAP>() as i32;
    if unsafe { GetObjectW(bitmap, size, (&mut bmp as *mut BITMAP).cast()) } == 0 {
        return None;
    }
    let (width, height) = (bmp.bmWidth, bmp.bmHeight);
    if width <= 0 || height <= 0 {
        return None;
    }

    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = std::mem::size_of_val(&info.bmiHeader) as u32;
    info.bmiHeader.biWidth = width;
    // Negative height asks for top-down rows.
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let lines = unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let lines = GetDIBits(dc, bitmap, 0, height as u32, pixels.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
        ReleaseDC(std::ptr::null_mut(), dc);
        lines
    };
    if lines == 0 {
        return None;
    }

    // GDI returns BGRA; old icons without an alpha channel come back fully transparent.
    let no_alpha = pixels.chunks_exact(4).all(|p| p[3] == 0);
    for p in pixels.chunks_exact_mut(4) {
        p.swap(0, 2);
        if no_alpha {
            p[3] = 255;
        }
    }
    Some((width as u32, height as u32, pixels))
}

/// NUL-terminated UTF-16 for Win32 APIs.
#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
