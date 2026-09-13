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

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn os_name() -> &'static str {
    "unknown"
}

/// Total system memory in MB, best-effort — `0` on read failure or an
/// unsupported platform. Purely informational (shown in the node info bar
/// alongside hostname/OS/arch/CPU count); not tied to any pressure threshold
/// (the watchdog's memory-pressure intervention that once used this kind of
/// reading was removed).
#[cfg(target_os = "macos")]
pub fn total_memory_mb() -> u64 {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map_or(0, |bytes| bytes / (1024 * 1024))
}

#[cfg(target_os = "linux")]
pub fn total_memory_mb() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|content| parse_meminfo_total_kb(&content))
        .map_or(0, |kb| kb / 1024)
}

#[cfg(target_os = "linux")]
fn parse_meminfo_total_kb(content: &str) -> Option<u64> {
    content.lines().find_map(|line| {
        line.strip_prefix("MemTotal:")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const fn total_memory_mb() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_name_is_known() {
        let name = os_name();
        assert!(
            ["macos", "linux", "wsl2", "unknown"].contains(&name),
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

    #[test]
    fn test_total_memory_mb_is_sane() {
        let mb = total_memory_mb();
        // On macOS/Linux this reads the real machine; on other platforms the
        // fallback returns 0. Either way it must not panic.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(mb > 0);
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert_eq!(mb, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_meminfo_total_kb() {
        let content = "MemTotal:       16384000 kB\nMemFree:         4096000 kB\n";
        assert_eq!(parse_meminfo_total_kb(content), Some(16_384_000));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_meminfo_total_kb_missing() {
        let content = "MemFree:         4096000 kB\n";
        assert_eq!(parse_meminfo_total_kb(content), None);
    }
}
