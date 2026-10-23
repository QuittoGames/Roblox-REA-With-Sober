use ash::{vk, Entry};
use serde::Serialize;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulong, c_void};

#[derive(Debug, Clone, Serialize, Default)]
pub struct VulkanProbe {
    pub loader: Option<String>,
    pub loaded: bool,
    pub instance_version: Option<String>,
    pub instance_extensions: Vec<String>,
    pub devices: Vec<DeviceProbe>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeviceProbe {
    pub name: String,
    pub device_type: String,
    pub api_version: Option<String>,
    pub driver_version: Option<String>,
    pub vendor_id: u32,
    pub device_id: u32,
    pub unified_memory: bool,
    pub pipeline_cache_uuid: Option<String>,
    pub heaps: Vec<HeapProbe>,
    pub queue_families: Vec<QueueFamilyProbe>,
    pub extensions: Vec<String>,
    pub surface: Option<SurfaceProbe>,
    pub surface_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeapProbe {
    pub index: u32,
    pub size: u64,
    pub device_local: bool,
    pub host_visible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueFamilyProbe {
    pub index: u32,
    pub queue_count: u32,
    pub graphics: bool,
    pub compute: bool,
    pub transfer: bool,
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SurfaceProbe {
    pub backend: String,
    pub chosen_format: Option<String>,
    pub chosen_color_space: Option<String>,
    pub present_modes: Vec<String>,
    pub current_extent: Option<String>,
    pub min_image_count: Option<u32>,
    pub max_image_count: Option<u32>,
    pub preferred_image_count: Option<u32>,
    pub composite_alpha: Option<String>,
    pub usage: Option<String>,
}

pub fn probe() -> VulkanProbe {
    let mut out = VulkanProbe::default();
    let (entry, loader) = match load_entry() {
        Ok(v) => v,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    out.loaded = true;
    out.loader = Some(loader);
    out.instance_version = unsafe { entry.try_enumerate_instance_version() }
        .ok()
        .flatten()
        .map(version);
    out.instance_extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
        .map(|props| {
            props
                .iter()
                .map(|p| {
                    p.extension_name_as_c_str()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let xlib_available = out
        .instance_extensions
        .iter()
        .any(|e| e == "VK_KHR_xlib_surface");
    let app_name = c"hardness-gfx-probe";
    let engine_name = c"hardness";
    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name)
        .application_version(vk::make_api_version(0, 0, 1, 0))
        .engine_name(engine_name)
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_1);
    let enabled_names = if xlib_available {
        vec![
            vk::KHR_SURFACE_NAME.as_ptr(),
            vk::KHR_XLIB_SURFACE_NAME.as_ptr(),
        ]
    } else {
        Vec::new()
    };
    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&enabled_names);
    let instance = match unsafe { entry.create_instance(&create_info, None) } {
        Ok(instance) => instance,
        Err(e) => {
            out.error = Some(format!("vkCreateInstance failed: {e:?}"));
            return out;
        }
    };

    let surface_api = ash::khr::surface::Instance::new(&entry, &instance);

    let physical_devices = match unsafe { instance.enumerate_physical_devices() } {
        Ok(devices) => devices,
        Err(e) => {
            out.error = Some(format!("vkEnumeratePhysicalDevices failed: {e:?}"));
            unsafe { instance.destroy_instance(None) };
            return out;
        }
    };

    for physical_device in physical_devices {
        let props = unsafe { instance.get_physical_device_properties(physical_device) };
        let memory = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        let queues =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let extensions = unsafe { instance.enumerate_device_extension_properties(physical_device) }
            .map(|props| {
                props
                    .iter()
                    .map(|p| {
                        p.extension_name_as_c_str()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let unified_memory = memory.memory_types[..memory.memory_type_count as usize]
            .iter()
            .any(|t| {
                t.property_flags.contains(
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
            });
        let heaps = memory.memory_heaps[..memory.memory_heap_count as usize]
            .iter()
            .enumerate()
            .map(|(index, heap)| {
                let host_visible = memory.memory_types[..memory.memory_type_count as usize]
                    .iter()
                    .any(|t| {
                        t.heap_index as usize == index
                            && t.property_flags
                                .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
                    });
                HeapProbe {
                    index: index as u32,
                    size: heap.size,
                    device_local: heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL),
                    host_visible,
                }
            })
            .collect();
        let queue_families = queues
            .iter()
            .enumerate()
            .map(|(index, q)| QueueFamilyProbe {
                index: index as u32,
                queue_count: q.queue_count,
                graphics: q.queue_flags.contains(vk::QueueFlags::GRAPHICS),
                compute: q.queue_flags.contains(vk::QueueFlags::COMPUTE),
                transfer: q.queue_flags.contains(vk::QueueFlags::TRANSFER),
                present: false,
            })
            .collect();
        let mut device = DeviceProbe {
            name: cstr_array(&props.device_name),
            device_type: format!("{:?}", props.device_type),
            api_version: Some(version(props.api_version)),
            driver_version: Some(version(props.driver_version)),
            vendor_id: props.vendor_id,
            device_id: props.device_id,
            unified_memory,
            pipeline_cache_uuid: Some(hex(&props.pipeline_cache_uuid)),
            heaps,
            queue_families,
            extensions,
            surface: None,
            surface_error: None,
        };
        if !xlib_available {
            device.surface_error = Some("VK_KHR_xlib_surface not advertised".to_string());
        } else {
            match create_xlib_surface(&entry, &instance, &surface_api, physical_device) {
                Ok((surface, mut surface_probe)) => {
                    for family in &mut device.queue_families {
                        family.present = unsafe {
                            surface_api.get_physical_device_surface_support(
                                physical_device,
                                family.index,
                                surface,
                            )
                        }
                        .unwrap_or(false);
                    }
                    surface_probe.backend = "xlib-xwayland".to_string();
                    device.surface = Some(surface_probe);
                    unsafe { surface_api.destroy_surface(surface, None) };
                }
                Err(e) => device.surface_error = Some(e),
            }
        }
        out.devices.push(device);
    }

    unsafe { instance.destroy_instance(None) };
    out
}

fn load_entry() -> Result<(Entry, String), String> {
    let candidates = [
        "libvulkan.so.1",
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
    ];
    for candidate in candidates {
        // SAFETY: loading the Vulkan loader is required to query the device.
        match unsafe { Entry::load_from(candidate) } {
            Ok(entry) => return Ok((entry, candidate.to_string())),
            Err(_) => continue,
        }
    }
    Err("cannot load libvulkan.so.1".to_string())
}

fn create_xlib_surface(
    entry: &Entry,
    instance: &ash::Instance,
    surface_api: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<(vk::SurfaceKHR, SurfaceProbe), String> {
    let display = XlibDisplay::open()?;
    let screen = unsafe { XDefaultScreen(display.0) };
    let root = unsafe { XRootWindow(display.0, screen) };
    let window = unsafe { XCreateSimpleWindow(display.0, root, 0, 0, 320, 200, 0, 0, 0) };
    if window == 0 {
        return Err("XCreateSimpleWindow returned 0".to_string());
    }
    unsafe {
        XMapWindow(display.0, window);
        XFlush(display.0);
    }
    let loader = ash::khr::xlib_surface::Instance::new(entry, instance);
    let create_info = vk::XlibSurfaceCreateInfoKHR::default()
        .dpy(display.0 as *mut vk::Display)
        .window(window);
    let surface = unsafe { loader.create_xlib_surface(&create_info, None) }
        .map_err(|e| format!("vkCreateXlibSurfaceKHR failed: {e:?}"))?;
    let probe = inspect_surface(surface_api, physical_device, surface);
    unsafe {
        XDestroyWindow(display.0, window);
        XFlush(display.0);
    }
    Ok((surface, probe))
}

fn inspect_surface(
    surface_api: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> SurfaceProbe {
    let mut out = SurfaceProbe::default();
    let formats =
        unsafe { surface_api.get_physical_device_surface_formats(physical_device, surface) }
            .unwrap_or_default();
    if let Some(format) = formats.first() {
        out.chosen_format = Some(format!("{:?}", format.format));
        out.chosen_color_space = Some(format!("{:?}", format.color_space));
    }
    let modes =
        unsafe { surface_api.get_physical_device_surface_present_modes(physical_device, surface) }
            .unwrap_or_default();
    out.present_modes = modes.iter().map(|m| format!("{m:?}")).collect();
    if let Ok(caps) =
        unsafe { surface_api.get_physical_device_surface_capabilities(physical_device, surface) }
    {
        out.current_extent = Some(format!(
            "{}x{}",
            caps.current_extent.width, caps.current_extent.height
        ));
        out.min_image_count = Some(caps.min_image_count);
        out.max_image_count = (caps.max_image_count > 0).then_some(caps.max_image_count);
        out.preferred_image_count = Some(if caps.max_image_count > 0 {
            caps.min_image_count.max(caps.max_image_count.min(4))
        } else {
            caps.min_image_count.max(3)
        });
        out.composite_alpha = Some(format!("{:?}", caps.supported_composite_alpha));
        out.usage = Some(format!("{:?}", caps.supported_usage_flags));
    }
    out
}

fn version(value: u32) -> String {
    format!(
        "{}.{}.{}",
        value >> 22,
        (value >> 12) & 0x3ff,
        value & 0xfff
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cstr_array(value: &[c_char]) -> String {
    // SAFETY: Vulkan guarantees a NUL-terminated deviceName buffer.
    unsafe { CStr::from_ptr(value.as_ptr()) }
        .to_string_lossy()
        .to_string()
}

struct XlibDisplay(*mut c_void);

impl XlibDisplay {
    fn open() -> Result<Self, String> {
        let display = CString::new("").unwrap();
        let ptr = unsafe { XOpenDisplay(display.as_ptr()) };
        if ptr.is_null() {
            Err("XOpenDisplay failed (DISPLAY unavailable)".to_string())
        } else {
            Ok(Self(ptr))
        }
    }
}

impl Drop for XlibDisplay {
    fn drop(&mut self) {
        unsafe { XCloseDisplay(self.0) };
    }
}

type Display = c_void;
type Window = c_ulong;

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XDefaultScreen(display: *mut Display) -> c_int;
    fn XRootWindow(display: *mut Display, screen: c_int) -> Window;
    fn XCreateSimpleWindow(
        display: *mut Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: std::os::raw::c_uint,
        height: std::os::raw::c_uint,
        border_width: std::os::raw::c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> Window;
    fn XMapWindow(display: *mut Display, window: Window) -> c_int;
    fn XFlush(display: *mut Display) -> c_int;
    fn XDestroyWindow(display: *mut Display, window: Window) -> c_int;
    fn XCloseDisplay(display: *mut Display) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_vulkan_version() {
        let v = vk::make_api_version(0, 1, 4, 335);
        assert_eq!(version(v), "1.4.335");
    }
}
