use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=dashboard/src");
    println!("cargo:rerun-if-changed=dashboard/index.html");
    println!("cargo:rerun-if-changed=dashboard/package.json");

    let dashboard_dir = Path::new("dashboard");
    if !dashboard_dir.exists() {
        eprintln!("cargo:warning=dashboard/ not found, skipping frontend build");
        return;
    }

    let node_modules = dashboard_dir.join("node_modules");
    if !node_modules.exists() {
        eprintln!("cargo:warning=Installing dashboard dependencies...");
        let status = Command::new("npm")
            .arg("install")
            .current_dir(dashboard_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("cargo:warning=npm install failed with {s}, skipping frontend build");
                return;
            }
            Err(e) => {
                eprintln!("cargo:warning=npm not found ({e}), skipping frontend build");
                return;
            }
        }
    }

    eprintln!("cargo:warning=Building dashboard...");
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(dashboard_dir)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("cargo:warning=npm run build failed with {s}");
        }
        Err(e) => {
            eprintln!("cargo:warning=npm not found ({e}), skipping frontend build");
        }
    }
}
