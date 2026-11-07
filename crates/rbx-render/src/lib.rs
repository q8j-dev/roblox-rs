pub mod filemesh;
pub mod geometry;
pub mod gui;
pub mod gui_pass;
pub mod lighting;
pub mod offscreen;
pub mod renderer;
pub mod scene;
pub mod smooth;
pub mod terrain;
pub mod texture;

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
