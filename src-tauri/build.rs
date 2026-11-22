fn main() {
    println!("cargo:rerun-if-env-changed=BEATMAPS_UPDATE_ENDPOINT");
    println!("cargo:rerun-if-env-changed=BEATMAPS_UPDATE_PUBKEY");
    tauri_build::build();
}
