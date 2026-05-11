fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let png_path = "arrow_arrows_direction_rotate_sync_icon_193421.png";
        let ico_path = std::env::var("OUT_DIR").unwrap() + "/icon.ico";

        let img = image::open(png_path).expect("failed to open icon PNG");
        let img = img.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
        img.save(&ico_path).expect("failed to convert icon to ICO");

        let mut res = winresource::WindowsResource::new();
        res.set_icon(&ico_path);
        res.compile().unwrap();
    }
}
