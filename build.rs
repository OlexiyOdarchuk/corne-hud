// Embeds the app icon and version info into the Windows executable. Does nothing on other targets.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packaging/icons/corne-hud.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("packaging/icons/corne-hud.ico")
        .set("ProductName", "Corne HUD")
        .set("FileDescription", "Corne HUD")
        .set("LegalCopyright", "MIT License");
    if let Err(e) = res.compile() {
        println!("cargo:warning=failed to embed the Windows icon: {e}");
    }
}
