use anyhow::Result;

/// A snapshot of system memory at a point in time.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub available_mb: u64,
    pub total_mb: u64,
}

impl MemorySnapshot {
    /// Returns memory usage as a percentage (0–100).
    pub fn usage_percent(&self) -> u8 {
        if self.total_mb == 0 {
            return 0;
        }
        let used = self.total_mb.saturating_sub(self.available_mb);
        let pct = (used * 100) / self.total_mb;
        u8::try_from(pct.min(100)).unwrap_or(100)
    }
}

/// Trait for reading system memory information.
pub trait MemoryReader: Send + Sync {
    fn read_memory(&self) -> Result<MemorySnapshot>;
}

/// Real system memory reader using platform-specific commands.
pub struct SystemMemoryReader;

impl MemoryReader for SystemMemoryReader {
    fn read_memory(&self) -> Result<MemorySnapshot> {
        read_system_memory()
    }
}

#[cfg(target_os = "macos")]
fn read_system_memory() -> Result<MemorySnapshot> {
    let total_output = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()?;
    let vm_output = std::process::Command::new("vm_stat").output()?;

    let sysctl_text = String::from_utf8_lossy(&total_output.stdout);
    let vm_text = String::from_utf8_lossy(&vm_output.stdout);
    parse_macos_memory(&sysctl_text, &vm_text)
}

/// Parse macOS memory information from `sysctl` and `vm_stat` output.
#[cfg(target_os = "macos")]
fn parse_macos_memory(sysctl_text: &str, vm_text: &str) -> Result<MemorySnapshot> {
    let total_bytes: u64 = sysctl_text
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("cannot parse sysctl output {sysctl_text:?}: {e}"))?;
    let total_mb = total_bytes / (1024 * 1024);

    let page_size = parse_vm_stat_page_size(vm_text)?;
    let free_pages = parse_vm_stat_field(vm_text, "Pages free")?;
    let inactive_pages = parse_vm_stat_field(vm_text, "Pages inactive")?;
    let purgeable_pages = parse_vm_stat_field(vm_text, "Pages purgeable").unwrap_or(0);

    let available_bytes = (free_pages + inactive_pages + purgeable_pages) * page_size;
    let available_mb = available_bytes / (1024 * 1024);

    Ok(MemorySnapshot {
        available_mb,
        total_mb,
    })
}

#[cfg(target_os = "macos")]
fn parse_vm_stat_page_size(text: &str) -> Result<u64> {
    // First line: "Mach Virtual Memory Statistics: (page size of 16384 bytes)"
    let first_line = text
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty vm_stat output"))?;
    let size_str = first_line
        .split("page size of ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .ok_or_else(|| anyhow::anyhow!("cannot parse page size from: {first_line}"))?;
    size_str
        .trim_end_matches(')')
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid page size {size_str:?}: {e}"))
}

#[cfg(target_os = "macos")]
fn parse_vm_stat_field(text: &str, field: &str) -> Result<u64> {
    for line in text.lines() {
        if line.starts_with(field) {
            let raw = line
                .split(':')
                .nth(1)
                .ok_or_else(|| anyhow::anyhow!("no colon in field line: {line}"))?;
            // Strip whitespace, trailing dots, and any non-digit characters
            let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
            if digits.is_empty() {
                anyhow::bail!("no numeric value in field line: {line}");
            }
            return Ok(digits.parse()?);
        }
    }
    anyhow::bail!("field not found in vm_stat: {field}")
}

#[cfg(target_os = "linux")]
fn read_system_memory() -> Result<MemorySnapshot> {
    let content = std::fs::read_to_string("/proc/meminfo")?;

    let total_kb = parse_meminfo_field(&content, "MemTotal")?;
    let available_kb = parse_meminfo_field(&content, "MemAvailable")?;

    Ok(MemorySnapshot {
        available_mb: available_kb / 1024,
        total_mb: total_kb / 1024,
    })
}

#[cfg(target_os = "linux")]
fn parse_meminfo_field(content: &str, field: &str) -> Result<u64> {
    for line in content.lines() {
        if line.starts_with(field) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                return Ok(parts[1].parse()?);
            }
        }
    }
    anyhow::bail!("field not found in /proc/meminfo: {field}")
}

/// Fallback for Windows and other platforms — report unknown memory.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_system_memory() -> Result<MemorySnapshot> {
    Ok(MemorySnapshot {
        available_mb: 0,
        total_mb: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_percent_normal() {
        let snap = MemorySnapshot {
            available_mb: 2048,
            total_mb: 8192,
        };
        assert_eq!(snap.usage_percent(), 75);
    }

    #[test]
    fn test_usage_percent_zero_total() {
        let snap = MemorySnapshot {
            available_mb: 100,
            total_mb: 0,
        };
        assert_eq!(snap.usage_percent(), 0);
    }

    #[test]
    fn test_usage_percent_full() {
        let snap = MemorySnapshot {
            available_mb: 0,
            total_mb: 8192,
        };
        assert_eq!(snap.usage_percent(), 100);
    }

    #[test]
    fn test_usage_percent_empty() {
        let snap = MemorySnapshot {
            available_mb: 8192,
            total_mb: 8192,
        };
        assert_eq!(snap.usage_percent(), 0);
    }

    #[test]
    fn test_usage_percent_available_exceeds_total() {
        // Shouldn't happen but should not panic
        let snap = MemorySnapshot {
            available_mb: 10000,
            total_mb: 8192,
        };
        assert_eq!(snap.usage_percent(), 0);
    }

    #[test]
    fn test_usage_percent_rounding() {
        // 1000 available of 3000 total = 2000/3000 = 66.6...% → truncates to 66
        let snap = MemorySnapshot {
            available_mb: 1000,
            total_mb: 3000,
        };
        assert_eq!(snap.usage_percent(), 66);
    }

    #[test]
    fn test_memory_snapshot_debug() {
        let snap = MemorySnapshot {
            available_mb: 4096,
            total_mb: 16384,
        };
        let debug = format!("{snap:?}");
        assert!(debug.contains("4096"));
        assert!(debug.contains("16384"));
    }

    #[test]
    fn test_memory_snapshot_clone() {
        let snap = MemorySnapshot {
            available_mb: 2048,
            total_mb: 8192,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = snap.clone();
        assert_eq!(cloned.available_mb, 2048);
        assert_eq!(cloned.total_mb, 8192);
    }

    #[test]
    fn test_system_memory_reader_reads() {
        let reader = SystemMemoryReader;
        let result = reader.read_memory();
        // This test runs on the actual system — just verify it returns a sane value
        let snap = result.unwrap();
        // On macOS/Linux total_mb > 0; on Windows/other platforms the fallback returns 0
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(snap.total_mb > 0);
        assert!(snap.usage_percent() <= 100);
    }

    #[cfg(target_os = "macos")]
    mod macos_tests {
        use super::super::*;

        #[test]
        fn test_parse_vm_stat_page_size() {
            let text = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                         Pages free:                             1234.\n";
            assert_eq!(parse_vm_stat_page_size(text).unwrap(), 16384);
        }

        #[test]
        fn test_parse_vm_stat_page_size_4k() {
            let text = "Mach Virtual Memory Statistics: (page size of 4096 bytes)\n";
            assert_eq!(parse_vm_stat_page_size(text).unwrap(), 4096);
        }

        #[test]
        fn test_parse_vm_stat_page_size_empty() {
            let result = parse_vm_stat_page_size("");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_vm_stat_page_size_no_match() {
            let result = parse_vm_stat_page_size("Some other text");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_vm_stat_field() {
            let text = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                         Pages free:                              5000.\n\
                         Pages active:                           10000.\n\
                         Pages inactive:                          3000.\n\
                         Pages purgeable:                          500.\n";
            assert_eq!(parse_vm_stat_field(text, "Pages free").unwrap(), 5000);
            assert_eq!(parse_vm_stat_field(text, "Pages inactive").unwrap(), 3000);
            assert_eq!(parse_vm_stat_field(text, "Pages purgeable").unwrap(), 500);
        }

        #[test]
        fn test_parse_vm_stat_field_not_found() {
            let text = "Pages free:  100.\n";
            let result = parse_vm_stat_field(text, "Pages nonexistent");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_vm_stat_field_no_colon() {
            let text = "Pages free  no colon here\n";
            let result = parse_vm_stat_field(text, "Pages free");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_vm_stat_field_extra_whitespace() {
            // Some vm_stat versions may have varying whitespace
            let text = "Pages free:                              5000.  \n";
            assert_eq!(parse_vm_stat_field(text, "Pages free").unwrap(), 5000);
        }

        #[test]
        fn test_parse_vm_stat_field_no_trailing_dot() {
            // Value without trailing dot
            let text = "Pages free:  5000\n";
            assert_eq!(parse_vm_stat_field(text, "Pages free").unwrap(), 5000);
        }

        #[test]
        fn test_parse_vm_stat_field_empty_value() {
            let text = "Pages free:   \n";
            let result = parse_vm_stat_field(text, "Pages free");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_vm_stat_page_size_extra_whitespace() {
            let text = "Mach Virtual Memory Statistics: (page size of  16384  bytes)\n";
            assert_eq!(parse_vm_stat_page_size(text).unwrap(), 16384);
        }

        #[test]
        fn test_parse_vm_stat_page_size_trailing_paren() {
            // Some formats may end with closing paren attached
            let text = "Mach Virtual Memory Statistics: (page size of 16384)\n";
            assert_eq!(parse_vm_stat_page_size(text).unwrap(), 16384);
        }

        #[test]
        fn test_parse_vm_stat_page_size_invalid_number() {
            let text = "Mach Virtual Memory Statistics: (page size of abc bytes)\n";
            let result = parse_vm_stat_page_size(text);
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_macos_memory_valid() {
            let sysctl = "17179869184\n"; // 16384 MB
            let vm_stat = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                           Pages free:                              5000.\n\
                           Pages inactive:                          3000.\n\
                           Pages purgeable:                          500.\n";
            let snap = parse_macos_memory(sysctl, vm_stat).unwrap();
            assert_eq!(snap.total_mb, 16384);
            // (5000+3000+500)*16384 / 1024 / 1024 = 133 MB
            assert_eq!(snap.available_mb, 132);
        }

        #[test]
        fn test_parse_macos_memory_invalid_sysctl() {
            let result = parse_macos_memory("not-a-number\n", "");
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cannot parse sysctl")
            );
        }

        #[test]
        fn test_parse_macos_memory_empty_sysctl() {
            let result = parse_macos_memory("", "");
            assert!(result.is_err());
        }

        #[test]
        fn test_parse_macos_memory_invalid_vmstat() {
            let result = parse_macos_memory("17179869184\n", "garbage output\n");
            assert!(result.is_err());
        }
    }

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::super::*;

        #[test]
        fn test_parse_meminfo_field() {
            let content = "MemTotal:       16384000 kB\n\
                           MemFree:         4096000 kB\n\
                           MemAvailable:    8192000 kB\n";
            assert_eq!(
                parse_meminfo_field(content, "MemTotal").unwrap(),
                16_384_000
            );
            assert_eq!(
                parse_meminfo_field(content, "MemAvailable").unwrap(),
                8_192_000
            );
        }

        #[test]
        fn test_parse_meminfo_field_not_found() {
            let content = "MemTotal: 1000 kB\n";
            let result = parse_meminfo_field(content, "MemAvailable");
            assert!(result.is_err());
        }
    }
}
