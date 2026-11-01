fn main() {
    // Bildirim alanı kitaplıkları (libayatana-appindicator ve bağımlılıkları) .deb paketinde
    // /usr/lib/lynora içinde gelir; çalışma anında (dlopen) önce orada aranır. DT_RPATH
    // (RUNPATH değil) kullanılır: o kitaplıkların kendi bağımlılıkları da orada bulunur.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,--disable-new-dtags,-rpath,/usr/lib/lynora");
    }
    tauri_build::build()
}
