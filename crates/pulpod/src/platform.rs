#[cfg(target_os = "linux")]
pub fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
pub const fn is_wsl() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub const fn os_name() -> &'static str {
    "macos"
}

#[cfg(target_os = "linux")]
pub fn os_name() -> &'static str {
    if is_wsl() { "wsl2" } else { "linux" }
}

#[cfg(target_os = "windows")]
pub fn os_name() -> &'static str {
    "windows"
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn os_name() -> &'static str {
    "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_name_is_known() {
        let name = os_name();
        assert!(
            ["macos", "linux", "wsl2", "windows", "unknown"].contains(&name),
            "Unexpected OS name: {name}"
        );
    }

    #[test]
    fn test_is_wsl_returns_bool() {
        let result = is_wsl();
        // On macOS this is always false; on Linux it depends on WSL
        assert!(!result || cfg!(target_os = "linux"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_os_name_macos() {
        assert_eq!(os_name(), "macos");
    }
}
