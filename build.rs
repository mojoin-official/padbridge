//! Embeds the padbridge icon into the Windows executables, so File Explorer, shortcuts and the
//! taskbar show it before the app has even started. Does nothing on other systems.

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/d1-aurora.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icons/d1-aurora.ico");
            res.set("ProductName", "padbridge");
            res.set("FileDescription", "padbridge");
            if let Err(e) = res.compile() {
                // A missing resource compiler should not stop the build; the app still works,
                // it just shows the default file icon.
                println!("cargo:warning=could not embed the app icon: {e}");
            }
        }
    }
}
