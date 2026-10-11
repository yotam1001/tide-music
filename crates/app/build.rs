fn main() {
    println!("cargo:rerun-if-changed=../../ui/assets/tide.ico");
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
