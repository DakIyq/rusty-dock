fn main() {
    slint_build::compile("gui/main.slint").unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_icon();
    }
}

/// Embeds `assets/icon.ico` as icon resource 1: the exe's icon in Explorer, shortcuts and Task
/// Manager, which `platform::Tray` loads for the tray too.
fn embed_icon() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("assets").join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // Generated with an absolute path, since rc.exe and windres resolve relative ones differently.
    let rc = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("rusty_dock.rc");
    let path = icon.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
