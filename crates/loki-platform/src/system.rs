//! Portable `system_info()` via `sysinfo`. No hostname, username or identifiers.

use crate::{PlatformError, SystemInfo};

/// Reads OS, CPU, RAM and the system disk.
pub fn read_system_info() -> Result<SystemInfo, PlatformError> {
    let mut system =
        sysinfo::System::new().map_err(|error| PlatformError::Failed(error.to_string()))?;
    system.refresh_memory();
    system.refresh_cpu_all();
    let cpus = system.cpus();
    let cpu = cpus
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| String::from("unknown"));
    let root = if cfg!(windows) { "C:\\" } else { "/" };
    let disk = sysinfo::Disks::new_with_refreshed_list()
        .ok()
        .and_then(|disks| {
            disks
                .iter()
                .find(|disk| disk.mount_point() == std::path::Path::new(root))
                .map(|disk| (disk.total_space(), disk.available_space()))
        });
    Ok(SystemInfo {
        os: sysinfo::System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.to_owned()),
        arch: std::env::consts::ARCH.to_owned(),
        cpu,
        cpu_cores: cpus.len(),
        ram_total_bytes: system.total_memory(),
        ram_available_bytes: system.available_memory(),
        disk_total_bytes: disk.map(|(total, _)| total),
        disk_available_bytes: disk.map(|(_, available)| available),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plausible_values_without_identifying_fields() {
        let info = read_system_info().unwrap();
        assert!(info.ram_total_bytes > 0);
        assert!(info.cpu_cores > 0);
        let json = serde_json::to_string(&info).unwrap();
        for forbidden in ["host", "user", "serial"] {
            assert!(!json.to_lowercase().contains(forbidden), "{json}");
        }
    }
}
