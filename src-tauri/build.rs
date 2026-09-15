fn main() {
    println!("cargo:rerun-if-changed=src/wifi.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/wifi.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("grindlewald_wifi");
        for framework in ["Foundation", "CoreLocation", "CoreWLAN"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    tauri_build::build()
}
