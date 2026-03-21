fn main() {
    println!("cargo:rerun-if-changed=../../assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/app.ico");
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().expect("Icon path must be valid UTF-8"))
            .compile()
            .expect("Cannot embed the service product icon");
    }
}
