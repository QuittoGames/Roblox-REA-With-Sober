use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Structured facts extracted from a Sober log. Only values actually found in
/// the text are populated; the log itself remains the evidence.
#[derive(Debug, Clone, Serialize)]
pub struct SoberLog {
    pub path: PathBuf,
    pub size: u64,
    pub app_tag: Option<String>,
    pub gpu: Option<String>,
    pub mesa: Option<String>,
    pub vulkan_api: Option<String>,
    pub wsi_extensions: Vec<String>,
    pub device_name: Option<String>,
    pub driver_version: Option<String>,
    pub heap_size: Option<u64>,
    pub unified_memory: Option<bool>,
    pub video_memory: Option<u64>,
    pub swapchain_images: Option<u32>,
    pub present_mode: Option<u32>,
    pub format: Option<u32>,
    pub resolution: Option<String>,
    pub shader_pack: Option<String>,
    pub shader_count: Option<u32>,
    pub shaders_compiled: Option<u32>,
    pub pipeline_cache_bytes: Option<u64>,
    pub pipeline_cache_skipped: bool,
    pub create_vulkan: Option<bool>,
    pub create_opengl: Option<bool>,
}

impl SoberLog {
    pub fn read(path: impl AsRef<Path>) -> Result<SoberLog, String> {
        let path = path.as_ref();
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(parse(&text, path.to_path_buf(), size))
    }
}

pub fn parse(text: &str, path: PathBuf, size: u64) -> SoberLog {
    let mut out = SoberLog {
        path,
        size,
        app_tag: None,
        gpu: None,
        mesa: None,
        vulkan_api: None,
        wsi_extensions: Vec::new(),
        device_name: None,
        driver_version: None,
        heap_size: None,
        unified_memory: None,
        video_memory: None,
        swapchain_images: None,
        present_mode: None,
        format: None,
        resolution: None,
        shader_pack: None,
        shader_count: None,
        shaders_compiled: None,
        pipeline_cache_bytes: None,
        pipeline_cache_skipped: false,
        create_vulkan: None,
        create_opengl: None,
    };
    for line in text.lines() {
        if out.app_tag.is_none() {
            if let Some(tag) = after_marker(line, "Roblox") {
                if tag.contains("-x86_64-gnu") {
                    out.app_tag = Some(tag.trim().to_string());
                }
            }
            if let Some(idx) = line.find("s0") {
                let candidate = line[idx..].split_whitespace().next().unwrap_or("");
                if candidate.contains("-x86_64-gnu") {
                    out.app_tag = Some(candidate.to_string());
                }
            }
        }
        if out.gpu.is_none() {
            if let Some(v) = after_marker(line, "GPU:") {
                out.gpu = Some(v.trim().to_string());
            } else if let Some(v) = after_marker(line, "Vulkan Device: ") {
                // Current Sober format logs the device as
                // "Vulkan Device: Intel(R) HD Graphics 520 (SKL GT2)".
                let name = v.trim();
                if !name.starts_with("Vendor ")
                    && !name.starts_with("Driver ")
                    && !name.starts_with("API ")
                    && !name.is_empty()
                {
                    out.gpu = Some(name.to_string());
                }
            }
        }
        if out.mesa.is_none() {
            if let Some(idx) = line.find("Mesa ") {
                let candidate: String = line[idx..]
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                if candidate.len() > "Mesa ".len() {
                    out.mesa = Some(candidate);
                }
            }
        }
        if out.vulkan_api.is_none() {
            if let Some(idx) = line.find("Vulkan API ") {
                let candidate: String = line[idx + "Vulkan API ".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if !candidate.is_empty() {
                    out.vulkan_api = Some(candidate);
                }
            } else if let Some(v) = after_marker(line, "Vulkan Device: API ") {
                let candidate: String = v
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if !candidate.is_empty() {
                    out.vulkan_api = Some(candidate);
                }
            }
        }
        for token in line.split_whitespace() {
            let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if token.starts_with("VK_") && !out.wsi_extensions.iter().any(|e| e == token) {
                out.wsi_extensions.push(token.to_string());
            }
        }
        if out.driver_version.is_none() {
            if let Some(v) = after_marker(line, "driverVersion:") {
                out.driver_version = Some(v.trim().to_string());
            } else if let Some(v) = after_marker(line, "Vulkan Device: Driver ") {
                let candidate: String = v
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if !candidate.is_empty() {
                    out.driver_version = Some(candidate);
                }
            }
        }
        if out.heap_size.is_none() {
            if let Some(v) =
                number_after(line, "heapSize:").or_else(|| number_after(line, "heapSize = "))
            {
                out.heap_size = Some(v);
            }
        }
        if out.unified_memory.is_none() {
            if line.contains("unifiedMemory: true") || line.contains("unifiedMemory = true") {
                out.unified_memory = Some(true);
            } else if line.contains("unifiedMemory: false")
                || line.contains("unifiedMemory = false")
            {
                out.unified_memory = Some(false);
            }
        }
        if out.video_memory.is_none() {
            if let Some(v) =
                number_after(line, "videoMemory:").or_else(|| number_after(line, "videoMemory = "))
            {
                out.video_memory = Some(v);
            }
        }
        if line.contains("swapchain") || line.contains("Swapchain") {
            if out.swapchain_images.is_none() {
                out.swapchain_images = number_after(line, "images:")
                    .or_else(|| number_after(line, "swapchain images "))
                    .map(|v| v as u32);
            }
            if out.present_mode.is_none() {
                out.present_mode = number_after(line, "presentMode:")
                    .or_else(|| number_after(line, "present mode "))
                    .map(|v| v as u32);
            }
            if out.format.is_none() {
                out.format = number_after(line, "format:")
                    .or_else(|| number_after(line, "format "))
                    .map(|v| v as u32);
            }
            if out.resolution.is_none() {
                out.resolution = resolution_after(line);
            }
        }
        if out.shader_pack.is_none() {
            if let Some(idx) = line.find(" shaders from pack ") {
                // Current format: "Loaded 2991 shaders from pack vulkan_mobile variant default"
                let rest = &line[idx + " shaders from pack ".len()..];
                let name = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(':');
                if !name.is_empty() {
                    out.shader_pack = Some(name.to_string());
                    out.shader_count = number_before(line, " shaders from pack ").map(|v| v as u32);
                }
            } else if line.contains("shader pack") {
                // Legacy format: "shader pack vulkan_mobile: shaders: 2988"
                let rest = after_marker(line, "shader pack ").unwrap_or("");
                let name = rest.split(':').next().unwrap_or("").trim();
                if !name.is_empty() {
                    out.shader_pack = Some(name.to_string());
                    out.shader_count = number_after(line, "shaders:").map(|v| v as u32);
                }
            }
        }
        if out.shaders_compiled.is_none()
            && line.contains("Compiled ")
            && line.contains(" shaders in ")
        {
            out.shaders_compiled = number_after(line, "Compiled ").map(|v| v as u32);
        }
        if out.pipeline_cache_bytes.is_none() {
            if let Some(v) = number_after(line, "pipeline cache (") {
                // Current format: "Skipping pipeline cache (274265 bytes) since validation failed"
                out.pipeline_cache_bytes = Some(v);
            } else if let Some(v) = number_after(line, "pipeline cache") {
                if line.contains("bytes") || v > 1024 {
                    out.pipeline_cache_bytes = Some(v);
                }
            }
        }
        if line.contains("Skipping pipeline cache") {
            out.pipeline_cache_skipped = true;
        }
        if line.contains("create_vulkan=true") {
            out.create_vulkan = Some(true);
        } else if line.contains("create_vulkan=false") {
            out.create_vulkan = Some(false);
        }
        if line.contains("create_opengl=true") {
            out.create_opengl = Some(true);
        } else if line.contains("create_opengl=false") {
            out.create_opengl = Some(false);
        }
    }
    out
}

fn after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|i| &line[i + marker.len()..])
}

fn number_after(line: &str, marker: &str) -> Option<u64> {
    let start = line.find(marker)? + marker.len();
    let digits: String = line[start..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn number_before(line: &str, marker: &str) -> Option<u64> {
    let end = line.find(marker)?;
    let digits: String = line[..end]
        .trim_end()
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse().ok()
}

fn resolution_after(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len()
                && bytes[i] == b'x'
                && i + 1 < bytes.len()
                && bytes[i + 1].is_ascii_digit()
            {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                return Some(line[start..j].to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vulkan_facts() {
        let text = r#"
[sober] GPU: Intel HD Graphics 520 (SKL GT2)
[sober] Vulkan API 1.4.335, Mesa 26.0.8
[sober] WSI: VK_KHR_surface VK_KHR_android_surface VK_KHR_get_physical_device_properties2
[sober] heapSize: 733090816, unifiedMemory: true, videoMemory: 67108864
[sober] swapchain: images: 4, presentMode: 2, format: 44, 1366x690
[sober] shader pack vulkan_mobile: shaders: 2988
[sober] create_vulkan=true create_opengl=false
"#;
        let log = parse(text, PathBuf::from("x.log"), 1);
        assert_eq!(log.vulkan_api.as_deref(), Some("1.4.335"));
        assert_eq!(log.heap_size, Some(733_090_816));
        assert_eq!(log.unified_memory, Some(true));
        assert_eq!(log.video_memory, Some(67_108_864));
        assert_eq!(log.swapchain_images, Some(4));
        assert_eq!(log.present_mode, Some(2));
        assert_eq!(log.format, Some(44));
        assert_eq!(log.resolution.as_deref(), Some("1366x690"));
        assert_eq!(log.shader_count, Some(2988));
        assert_eq!(log.create_vulkan, Some(true));
        assert!(log
            .wsi_extensions
            .contains(&"VK_KHR_android_surface".to_string()));
    }

    #[test]
    fn does_not_invent_missing_values() {
        let log = parse("nothing relevant", PathBuf::from("x.log"), 0);
        assert!(log.vulkan_api.is_none());
        assert!(log.swapchain_images.is_none());
    }

    #[test]
    fn parses_current_sober_log_format() {
        // Lines copied verbatim from a real Sober game-session log (2026-10-23).
        let text = "\
info: window: Will create window; create_hidpi=false, create_vulkan=true, create_opengl=false
info: Roblox: 2026-10-23T18:50:56.787Z [FLog::Graphics] Vulkan: Using extension: VK_KHR_surface
info: Roblox: 2026-10-23T18:50:56.787Z [FLog::Graphics] Vulkan: Using extension: VK_KHR_android_surface
info: Roblox: 2026-10-23T18:50:57.102Z,6.102426 [FLog::Graphics] ':Intel(R) HD Graphics 520 (SKL GT2):109060101' - PowerVR caps: vendorId=32902 supportsTextureMSAA=true
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] VULKAN heapIndex = 0, heapFlags = 1 (device), heapSize = 732285952
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] VULKAN unifiedMemory = true, device memory = 9322220544, host memory = 0, setting caps.videoMemory = 67108864
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] Vulkan Device: Intel(R) HD Graphics 520 (SKL GT2)
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] Vulkan Device: Vendor 8086 Device 1916
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] Vulkan Device: Driver 26.2.5 (109060101)
info: Roblox: 2026-10-23T18:50:57.102Z [FLog::Graphics] Vulkan Device: API 1.4.354
info: Roblox: 2026-10-23T18:50:57.107Z [FLog::Graphics] Vulkan: Skipping pipeline cache (274265 bytes) since validation failed
info: vulkan: create swapchain with w 1366 h 690
info: Roblox: 2026-10-23T18:50:57.115Z [FLog::Graphics] Vulkan: swapchain images 4 present mode 2 format 44 size 1366x690
info: Roblox: 2026-10-23T18:50:57.647Z [FLog::Graphics] Loaded 2991 shaders from pack vulkan_mobile variant default (17713871 bytes)
info: Roblox: 2026-10-23T18:50:57.647Z [FLog::Graphics] Compiled 1079 shaders in 529 ms
";
        let log = parse(text, PathBuf::from("latest.log"), 1);
        assert_eq!(log.create_vulkan, Some(true));
        assert_eq!(log.create_opengl, Some(false));
        assert_eq!(
            log.gpu.as_deref(),
            Some("Intel(R) HD Graphics 520 (SKL GT2)")
        );
        assert_eq!(log.vulkan_api.as_deref(), Some("1.4.354"));
        assert_eq!(log.driver_version.as_deref(), Some("26.2.5"));
        assert_eq!(log.heap_size, Some(732_285_952));
        assert_eq!(log.unified_memory, Some(true));
        assert_eq!(log.video_memory, Some(67_108_864));
        assert_eq!(log.swapchain_images, Some(4));
        assert_eq!(log.present_mode, Some(2));
        assert_eq!(log.format, Some(44));
        assert_eq!(log.resolution.as_deref(), Some("1366x690"));
        assert_eq!(log.shader_pack.as_deref(), Some("vulkan_mobile"));
        assert_eq!(log.shader_count, Some(2991));
        assert_eq!(log.shaders_compiled, Some(1079));
        assert_eq!(log.pipeline_cache_bytes, Some(274_265));
        assert!(log.pipeline_cache_skipped);
        assert!(log
            .wsi_extensions
            .contains(&"VK_KHR_android_surface".to_string()));
    }

    #[test]
    fn number_before_extracts_preceding_count() {
        assert_eq!(
            number_before("Loaded 2991 shaders from pack x", " shaders from pack "),
            Some(2991)
        );
        assert_eq!(number_before("no number here", " shaders from pack "), None);
    }
}
