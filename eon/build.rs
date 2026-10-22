//! Builds the approval page from `web/` with npm and embeds its files in the binary.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let web = manifest.join("web");
    for path in ["src", "static", "package.json", "package-lock.json", "svelte.config.js", "vite.config.ts"] {
        println!("cargo:rerun-if-changed={}", web.join(path).display());
    }
    let npm = |args: &[&str]| {
        let status = Command::new("npm")
            .args(args)
            .current_dir(&web)
            .env("EON_WEB_OUT", out.join("web"))
            .status()
            .unwrap_or_else(|error| panic!("the approval page needs Node 22+ and npm to build (npm: {error})"));
        assert!(status.success(), "npm {} failed in {}", args.join(" "), web.display());
    };
    if !web.join("node_modules").is_dir() {
        npm(&["ci", "--no-audit", "--no-fund"]);
    }
    let _ = std::fs::remove_dir_all(out.join("web"));
    npm(&["run", "build"]);

    let mut files = Vec::new();
    collect(&out.join("web"), &mut files);
    files.sort();
    let mut table = String::from("pub(super) const ASSETS: &[Asset] = &[\n");
    for file in files {
        let path = file.strip_prefix(out.join("web")).unwrap().to_str().expect("page file names are UTF-8");
        let kind = match file.extension().and_then(|value| value.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("json") => "application/json",
            Some("woff2") => "font/woff2",
            Some("svg") => "image/svg+xml",
            _ => "application/octet-stream",
        };
        table.push_str(&format!(
            "    Asset {{ path: {path:?}, kind: {kind:?}, body: include_bytes!({:?}) }},\n",
            file.display().to_string()
        ));
    }
    table.push_str("];\n");
    std::fs::write(out.join("web_assets.rs"), table).expect("writing the page table");
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("reading the page build") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
