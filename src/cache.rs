use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Read-only inspection of Sober's pipeline cache file.
///
/// Sober currently writes an `RPCV` container whose payload contains a
/// Vulkan `VkPipelineCacheHeaderVersionOne` (header size 32, version 1,
/// vendor/device, then the 16-byte pipeline-cache UUID). We report the
/// container and the embedded header without treating the cache as valid
/// merely because the file exists.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineCache {
    pub path: PathBuf,
    pub file_size: u64,
    pub magic: String,
    pub container_version: Option<u32>,
    pub container_declared_size: Option<u32>,
    pub header_offset: Option<usize>,
    pub header_size: Option<u32>,
    pub header_version: Option<u32>,
    pub vendor_id: Option<u32>,
    pub device_id: Option<u32>,
    pub uuid: Option<String>,
    pub format_valid: bool,
    pub matches_expected_device: Option<bool>,
}

pub fn inspect(
    path: impl AsRef<Path>,
    expected_vendor: Option<u32>,
    expected_device: Option<u32>,
) -> Result<Option<PipelineCache>, String> {
    let path = path.as_ref();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    Ok(Some(parse_bytes(
        path.to_path_buf(),
        &bytes,
        expected_vendor,
        expected_device,
    )))
}

fn parse_bytes(
    path: PathBuf,
    bytes: &[u8],
    expected_vendor: Option<u32>,
    expected_device: Option<u32>,
) -> PipelineCache {
    let magic = bytes
        .get(0..4)
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let is_container = magic == "RPCV";
    let header_offset = find_vulkan_header(bytes);
    let header = header_offset.map(|offset| &bytes[offset..]);
    let u32_at = |offset: usize| -> Option<u32> {
        header
            .and_then(|h| h.get(offset..offset + 4))
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // VkPipelineCacheHeaderVersionOne layout: headerSize(4) headerVersion(4)
    // vendorID(4) deviceID(4) pipelineCacheUUID(16) = 32 bytes total.
    let uuid = header.and_then(|h| h.get(16..32)).map(hex);
    let uuid_present = uuid.is_some();
    let vendor_id = u32_at(8);
    let device_id = u32_at(12);
    let header_size = u32_at(0);
    let header_version = u32_at(4);
    let matches_expected_device = match (vendor_id, device_id, expected_vendor, expected_device) {
        (Some(v), Some(d), Some(ev), Some(ed))
            if v <= u32::from(u16::MAX) && d <= u32::from(u16::MAX) =>
        {
            Some(v == ev && d == ed)
        }
        _ => None,
    };
    PipelineCache {
        path,
        file_size: bytes.len() as u64,
        magic,
        container_version: is_container.then(|| u32_at(12)).flatten(),
        container_declared_size: is_container.then(|| u32_at(4)).flatten(),
        header_offset,
        header_size,
        header_version,
        vendor_id,
        device_id,
        uuid,
        format_valid: header_size == Some(32) && header_version == Some(1) && uuid_present,
        matches_expected_device,
    }
}

/// Locate a plausible Vulkan pipeline-cache header. A raw cache starts at 0;
/// Sober's container places it later, so scan the first 128 bytes.
fn find_vulkan_header(bytes: &[u8]) -> Option<usize> {
    (0..bytes.len().saturating_sub(28)).find(|&offset| {
        let header_size = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let header_version = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        header_size == 32 && header_version == 1
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container() -> Vec<u8> {
        let mut bytes = vec![0u8; 80];
        bytes[0..4].copy_from_slice(b"RPCV");
        bytes[4..8].copy_from_slice(&123u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&32u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&1u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&0x8086u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&0x1916u32.to_le_bytes());
        bytes[64..80].fill(0xab);
        bytes
    }

    #[test]
    fn finds_embedded_header_and_device_match() {
        let cache = parse_bytes(
            PathBuf::from("cache.bin"),
            &container(),
            Some(0x8086),
            Some(0x1916),
        );
        assert_eq!(cache.magic, "RPCV");
        assert_eq!(cache.header_offset, Some(48));
        assert_eq!(cache.header_version, Some(1));
        assert_eq!(cache.vendor_id, Some(0x8086));
        assert_eq!(cache.device_id, Some(0x1916));
        assert!(cache.format_valid);
        assert_eq!(cache.matches_expected_device, Some(true));
    }

    #[test]
    fn reads_uuid_from_vk_header_layout() {
        // UUID lives at header bytes 16..32, after size/version/vendor/device.
        let cache = parse_bytes(PathBuf::from("cache.bin"), &container(), None, None);
        assert_eq!(
            cache.uuid.as_deref(),
            Some("abababababababababababababababab")
        );
    }

    #[test]
    fn detects_device_mismatch() {
        let cache = parse_bytes(
            PathBuf::from("cache.bin"),
            &container(),
            Some(0x8086),
            Some(0x1234),
        );
        assert_eq!(cache.matches_expected_device, Some(false));
    }

    #[test]
    fn empty_file_has_no_header() {
        let cache = parse_bytes(PathBuf::from("cache.bin"), &[], None, None);
        assert!(cache.header_offset.is_none());
        assert!(!cache.format_valid);
    }
}
