fn main() {
    println!("cargo:rerun-if-changed=../../ui/assets/tide.ico");
    // Desktop shells with a native platform integration crate (media controls,
    // global shortcuts, Mini pinning, Discord IPC): Windows, macOS and Linux.
    println!("cargo::rustc-check-cfg=cfg(native_shell)");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "windows" | "macos" | "linux") {
        println!("cargo:rustc-cfg=native_shell");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../ui/assets/tide.ico")
            .set("ProductName", "Tide Music")
            .set("FileDescription", "Tide Music Player")
            .compile()
            .expect("Could not embed app icon");
    }
    slint_build::compile("../../ui/main.slint").expect("Could not compile native interface");
}
