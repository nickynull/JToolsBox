use std::path::{Path, PathBuf};
use std::{env, fs};

fn collect(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, base, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            let url_path = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
            out.push((url_path, path.clone()));
        }
    }
}

fn module_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert_str(0, "app_");
    }
    name
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let web_root = manifest_dir.join("../web");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", web_root.display());

    // 每个 `web/<name>/index.html` 都是一个独立的网页应用，互不共享
    let mut apps: Vec<(String, String, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&web_root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() || !dir.join("index.html").is_file() {
                continue;
            }
            let raw = entry.file_name().to_string_lossy().to_string();
            apps.push((module_name(&raw), raw, dir));
        }
    }
    apps.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::new();
    for (module, raw, dir) in &apps {
        let dist = dir.join("dist");
        println!("cargo:rerun-if-changed={}", dist.display());
        println!("cargo:rerun-if-changed={}", dist.join("assets").display());

        let mut assets = Vec::new();
        collect(&dist, &dist, &mut assets);
        assets.sort_by(|a, b| a.0.cmp(&b.0));

        code.push_str(&format!("pub mod {} {{\n", module));
        code.push_str("    pub static ASSETS: &[(&str, &[u8])] = &[\n");
        for (url_path, file_path) in &assets {
            code.push_str(&format!(
                "        ({:?}, include_bytes!({:?})),\n",
                url_path,
                file_path.display().to_string()
            ));
        }
        code.push_str("    ];\n");

        let index = dist.join("index.html");
        if index.is_file() {
            code.push_str(&format!(
                "    pub static INDEX_HTML: &[u8] = include_bytes!({:?});\n",
                index.display().to_string()
            ));
        } else {
            let placeholder = format!(
                "<!DOCTYPE html><html lang=zh-CN><meta charset=utf-8><title>{raw}</title><body style=font-family:system-ui;background:%231e1e1e;color:%23ccc;display:flex;align-items:center;justify-content:center;height:100vh><div><h2>网页端尚未构建</h2><p>请在项目根目录运行 <code>npm run build:web:{raw}</code> 后重新编译。</p></div></body></html>"
            );
            code.push_str(&format!(
                "    pub static INDEX_HTML: &[u8] = {:?}.as_bytes();\n",
                placeholder
            ));
        }
        code.push_str("}\n\n");
    }

    if code.is_empty() {
        code.push_str("// 未发现 web/<name>/index.html 形式的网页应用\n");
    }

    let out_file = PathBuf::from(env::var("OUT_DIR").unwrap()).join("web_assets.rs");
    fs::write(out_file, code).unwrap();

    tauri_build::build()
}
