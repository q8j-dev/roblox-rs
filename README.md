# roblox-rs; a rewrite of roblox in rust

## technical info

- vmprotect, netpmc, and the golden hash checker are completely stripped out (thank you [aya](https://github.com/anice-guy/aya) for inspiration).
- wgpu is used for graphics, meaning it can render with vulkan, dx12, metal, opengl es, and webgpu/webgl.
- egui is used for the studio ui. it is very accurate to the original
- the server has also been rewritten in rust

## platforms

there is support for windows, linux, macos and wasm, with experimental mobile support that is untested
wasm builds are multithreaded

## ai disclosure

this is a highly experimental project made with claude opus 5.5. i did not read the code. this is just for fun, mainly.
