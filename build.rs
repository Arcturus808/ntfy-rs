fn main() {
    // Only embed Windows resources when building the binary, not the library.
    // When ntfy-rs is used as a library dependency (e.g. embedded in a Tauri app),
    // the host application provides its own version resource and icon.
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows"
        && std::env::var("CARGO_BIN_NAME").is_ok()
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/favicon.ico");
        res.compile().expect("failed to compile Windows resource");
    }
}
