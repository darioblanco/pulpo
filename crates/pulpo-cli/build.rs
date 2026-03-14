fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();

    let full = if hash.is_empty() {
        version.to_owned()
    } else {
        format!("{version} ({hash})")
    };
    println!("cargo:rustc-env=PULPO_VERSION={full}");
    // Re-run when HEAD changes (new commits)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads/");
}
