fn main() {
    println!("cargo:rerun-if-changed=app_icon.ico");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    // Build scripts run on the host; cfg!(windows) would inspect the wrong target
    // when cross-compiling. Query Cargo's target environment instead.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::path::Path::new("app_icon.ico").is_file() {
        // Local source-only builds must not require a CI-downloaded icon.
        println!("cargo:warning=app_icon.ico is absent; building without an application icon");
        return;
    }
    let mut resource = winres::WindowsResource::new();
    resource.set_icon("app_icon.ico");
    resource
        .compile()
        .expect("Windows resource compilation failed; check the icon and Windows SDK");
}
