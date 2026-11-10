fn main() {
    println!("cargo:rerun-if-changed=../../assets/grove/logo.svg");
    let svg = std::fs::read("../../assets/grove/logo.svg").expect("Grove logo");
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).expect("Valid Grove SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(256, 256).expect("Icon pixels");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(256.0 / 540.0, 256.0 / 540.0),
        &mut pixmap.as_mut(),
    );
    let icon = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("grove.ico");
    image::save_buffer_with_format(
        &icon,
        pixmap.data(),
        256,
        256,
        image::ColorType::Rgba8,
        image::ImageFormat::Ico,
    )
    .expect("Grove ICO");
    println!("cargo:rustc-env=GROVE_BUILT_ICON={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().unwrap())
            .set("ProductName", "Grove")
            .set("FileDescription", "Grove Spotify Music Player")
            .set("ProductVersion", env!("CARGO_PKG_VERSION"))
            .compile()
            .expect("Grove Windows resources");
    }
    slint_build::compile("../../ui/grove.slint").expect("Could not compile Grove interface");
}
