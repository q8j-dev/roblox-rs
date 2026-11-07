pub mod adorn;
pub mod adorn_pass;
pub mod celestial;
pub mod character;
pub mod composite;
pub mod filemesh;
pub mod geometry;
pub mod gui;
pub mod gui_pass;
pub mod lightgrid;
pub mod lightgrid_terrain;
pub mod lighting;
pub mod name_tags;
pub mod offscreen;
pub mod particle_pass;
pub mod particles;
pub mod renderer;
pub mod scene;
pub mod shadow;
pub mod smooth;
pub mod terrain;
pub mod texture;
pub mod world_gui;

pub use offscreen::Offscreen;
pub use renderer::Renderer;
pub use wgpu;

pub fn platform_backends() -> wgpu::Backends {
    if cfg!(target_arch = "wasm32") {
        wgpu::Backends::BROWSER_WEBGPU
    } else if cfg!(windows) {
        wgpu::Backends::DX12
    } else if cfg!(target_vendor = "apple") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::VULKAN
    }
}

pub fn create_instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = platform_backends();
    wgpu::Instance::new(desc.with_env())
}

pub fn native_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    if cfg!(target_arch = "wasm32") {
        return wgpu::Features::empty();
    }
    adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC
}

pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let info = adapter.get_info();
    tracing::info!(
        "graphics: {:?} on {} ({:?}), features {:?}",
        info.backend,
        info.name,
        info.device_type,
        native_features(adapter)
    );
    wgpu::DeviceDescriptor {
        label: Some("rbx-render"),
        required_features: native_features(adapter),
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        ..Default::default()
    }
}
