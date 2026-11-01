use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rerun-if-env-changed=SMARTTHERM_REQUIRE_WEB");
    let source = PathBuf::from("web/dist");
    let destination = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("assets");
    if destination.exists() {
        fs::remove_dir_all(&destination).unwrap();
    }
    fs::create_dir_all(&destination).unwrap();
    if source.join("index.html").exists() {
        copy(&source, &destination);
    } else {
        assert!(
            env::var_os("SMARTTHERM_REQUIRE_WEB").is_none(),
            "Frontend missing. Run make build (or npm ci && npm run build in web/) first."
        );
        fs::write(destination.join("index.html"),
            "<!doctype html><title>SmartThermWeb development</title><h1>Backend ready</h1><p>Run make dev for the dashboard, or make build for embedded assets.</p>").unwrap();
    }
    println!(
        "cargo:rustc-env=SMARTTHERM_ASSETS={}",
        destination.display()
    );
}

fn copy(source: &std::path::Path, destination: &std::path::Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
