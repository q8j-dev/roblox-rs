# roblox-rs; a port of roblox to rust

## technical info

- vmprotect, netpmc, and the golden hash checker are completely stripped out.
- wgpu is used for graphics, so it can run on vulkan, dx12, metal, opengl es, and webgpu/webgl.
- egui is used for the studio ui.

## platforms

there is support for windows, linux, macos and wasm, with experimental mobile support that is untested

wasm builds are multithreaded

## ai disclosure

this is a highly experimental project made with claude opus 5.5. i did not read the code. this is just for fun, mainly.
