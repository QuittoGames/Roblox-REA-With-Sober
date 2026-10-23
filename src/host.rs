use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Host facts that influence the graphics path. All fields are best-effort:
/// missing diagnostics are represented as `None` rather than guessed.
#[derive(Debug, Clone, Serialize, Default)]
pub struct HostInfo {
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_flags: Vec<String>,
    pub memory_total_kib: Option<u64>,
    pub memory_available_kib: Option<u64>,
    pub gpus: Vec<GpuInfo>,
    pub mesa: Option<MesaInfo>,
    pub vulkan_loader: Option<String>,
    pub flatpak_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GpuInfo {
    pub card: String,
    pub pci_id: Option<String>,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub driver: Option<String>,
    pub boot_vga: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MesaInfo {
    pub renderer: Option<String>,
    pub version: Option<String>,
    pub core_version: Option<String>,
}

pub fn collect() -> HostInfo {
    let cpu = cpu_info();
    HostInfo {
        os: os_release(),
        kernel: read_trim("/proc/sys/kernel/osrelease"),
        session_type: env_nonempty("XDG_SESSION_TYPE"),
        desktop: env_nonempty("XDG_CURRENT_DESKTOP").or_else(|| env_nonempty("DESKTOP_SESSION")),
        cpu_model: cpu.0,
        cpu_flags: cpu.1,
        memory_total_kib: meminfo("MemTotal"),
        memory_available_kib: meminfo("MemAvailable"),
        gpus: gpus(),
        mesa: mesa_info(),
        vulkan_loader: vulkan_loader(),
        flatpak_version: command_stdout("flatpak", &["--version"]),
    }
}

fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn os_release() -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn cpu_info() -> (Option<String>, Vec<String>) {
    let Ok(text) = fs::read_to_string("/proc/cpuinfo") else {
        return (None, Vec::new());
    };
    let mut model = None;
    let mut flags = Vec::new();
    for line in text.lines() {
        if model.is_none() {
            if let Some(value) = line.strip_prefix("model name") {
                model = Some(value.trim_start_matches([':', ' ', '\t']).to_string());
            }
        }
        if let Some(value) = line.strip_prefix("flags") {
            let set: Vec<String> = value
                .trim_start_matches([':', ' ', '\t'])
                .split_whitespace()
                .filter(|f| matches!(*f, "sse4_1" | "sse4_2" | "avx" | "avx2" | "avx512f"))
                .map(str::to_string)
                .collect();
            if !set.is_empty() {
                flags = set;
            }
        }
    }
    (model, flags)
}

fn meminfo(key: &str) -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start_matches(':').trim();
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn gpus() -> Vec<GpuInfo> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        if !device.join("vendor").exists() {
            continue;
        }
        let uevent = fs::read_to_string(device.join("uevent")).unwrap_or_default();
        let mut pci_id = None;
        let mut driver = None;
        for line in uevent.lines() {
            if let Some(v) = line.strip_prefix("PCI_ID=") {
                pci_id = Some(v.to_string());
            }
            if let Some(v) = line.strip_prefix("DRIVER=") {
                driver = Some(v.to_string());
            }
        }
        let gpu = GpuInfo {
            card: name,
            pci_id,
            vendor_id: read_trim(device.join("vendor"))
                .map(|v| v.trim_start_matches("0x").to_string()),
            device_id: read_trim(device.join("device"))
                .map(|v| v.trim_start_matches("0x").to_string()),
            driver: driver.or_else(|| {
                fs::read_link(device.join("driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            }),
            boot_vga: read_trim(device.join("boot_vga")).map(|v| v == "1"),
        };
        if !out
            .iter()
            .any(|g: &GpuInfo| g.pci_id == gpu.pci_id && g.pci_id.is_some())
        {
            out.push(gpu);
        }
    }
    out.sort_by(|a, b| a.card.cmp(&b.card));
    out
}

fn mesa_info() -> Option<MesaInfo> {
    let text = command_stdout("glxinfo", &["-B"])?;
    let mut info = MesaInfo::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("OpenGL renderer string:") {
            info.renderer = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("OpenGL core profile version string:") {
            info.core_version = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("OpenGL version string:") {
            info.version = Some(v.trim().to_string());
        }
    }
    (info.renderer.is_some() || info.version.is_some()).then_some(info)
}

fn vulkan_loader() -> Option<String> {
    for path in [
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/lib64/libvulkan.so.1",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            let resolved = fs::canonicalize(&p).unwrap_or(p);
            return Some(resolved.to_string_lossy().to_string());
        }
    }
    None
}
