pub fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        if is_wsl() { "wsl2" } else { "linux" }
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}
