# Future lighting: what Studio does and how to port it

Evidence comes from the Windows Studio build in `builds/windows-studio-version-67c8b1dedbb74fed` (`RobloxStudioBeta.exe`, client 0.746). Function addresses below are virtual addresses in that exe (image base `0x140000000`). Shader references are indices into the Vulkan pack (`shaders_vulkan_desktop.pack`) after splitting it at SPIR-V headers and decompiling with spirv-cross, and into the GLSL3 pack where the struct member names survive.

## How the pieces fit

Future is not a replacement for the voxel light grid. `LightGridUnified` (RTTI `.?AULightGridUnified@Graphics@RBX@@`, vftable `0x149351bd0`) holds two objects and forwards every virtual call to both:

- `+0x10`: the CPU voxel grid (`LightGridCPU`, vftable `0x1493771d8`). It still produces `LightMapTexture` and `LightGridSkylightTexture`, the two 3D textures every Future shader samples for ambient and sky occlusion. This is the grid we already port in `crates/rbx-render/src/lightgrid.rs`.
- `+0x18`: the GPU froxel light culler (the "LightGridFSM" object). Its buffers and textures carry debug names such as `LightGridFSM Lights`, `LightGridFSM Cull`, `LightGridFSM Cull Chunk` and `LightGridFSM Tile 3D`.

So a Future frame is: update the voxel grid as before, then pack and cull the local lights into a froxel grid, then shade each pixel with ambient from the voxel grid plus a loop over the froxel's light list.

## Per-frame CPU work (`0x144b166d0`)

1. Read the viewport size W×H.
2. If there are more lights than the cap at `+0x1bc`, sort them by squared distance to the camera and keep the nearest. The GPU buffers are sized to `cap × 80` bytes, and the shader declares 819 lights, which is exactly one 64 KB constant buffer.
3. Build `FroxelTransform = scale(W/32, H/32) · translate(0.5, 0.5) · scale(0.5, −0.5) · ViewProjection` and write it into the globals. Tiles are 32×32 pixels.
4. Resize the grid if the viewport changed (`0x144b15190`): the tile texture is ceil(W/32) × ceil(H/32) × 33 slices (13 on the CPU culling path), and the index texture is ceil(W/32) × ceil(H/32) × 2048, both 32-bit signed integer.
5. Pack lights (`0x144b16ec0`) and cull them, on the GPU (`0x144b13bc0`) or on the CPU when `DebugForceFSMCPULightCulling` is set (`0x144b12c30`).

## Light records

Each light gets two records.

The cull record (48 bytes, view space), built from two `LightObject` methods (`0x144ae31a0` for the AABB, `0x144ae41b0` for the sphere):

| field | meaning |
|---|---|
| vec3 + pad | AABB min |
| vec3 + pad | AABB max |
| vec3 | bounding sphere center |
| float | bounding sphere radius |

Every 32 lights also get a chunk record: the union AABB of that group, so the culler can reject 32 lights with one test.

Bounding spheres by light type (`LightObject +0x150`: 1 point, 2 spot, 3 surface):

- Point: center = position, radius = range.
- Spot with angle θ ≤ 90°: radius = range / (2·cos(θ/2)), center = position + direction·radius.
- Spot with θ > 90°: center = position + direction·cos(θ/2)·range, radius = sin(θ/2)·range.
- Surface: built from the two face axes and half-extents.

`LightObject` fields used: position `+0x118`, direction `+0x124`, range `+0x164`, spot angle in degrees `+0x168`, surface axes `+0x130` and `+0x140` with half-extents `+0x13c` and `+0x14c`.

The shading record (80 bytes, world space), as read by every lit shader (Vulkan shader 1014 is a small example):

| field | meaning |
|---|---|
| vec3 `pos`, float `rangePacked` | position; range = floor(rangePacked)·0.001, falloff start = fract(rangePacked)·range |
| vec3 `color`, int `shadow` | color times brightness; shadow slot or −1 |
| vec3 `dir`, float `spotCos` | spot direction; cosine of the half angle, 0 for point lights |
| vec4 `axisU` | surface light axis and half-extent (w) |
| vec4 `axisV` | second surface light axis and half-extent (w) |

## Culling (`LightGridCullCS`, Vulkan shader 0221)

Constant block (288 bytes, uploaded by `0x144b13bc0`): projection, inverse projection scaled to tile space, grid size (uvec3), light count, a flags int, and 8 spare vec4.

One thread per screen tile (8×8 groups). For each depth slice z of the tile column:

- Slice depth range in view space: slice 0 spans the near plane to 10 studs; slice k spans 10·1.5^(k−1) to 10·1.5^k studs.
- The froxel gets a view-space AABB from its eight corners, and a cone: axis through the tile center, depth range [min, max] along the axis, and a spread factor (the largest corner distance from the axis divided by its depth).
- Lights are tested in groups of 32. A group is skipped if its chunk AABB misses the froxel AABB. Otherwise each light passes if its AABB overlaps the froxel AABB and its sphere reaches the cone: with t = clamp(dot(center, axis), dmin, dmax), the distance d1 = max(0, |center − axis·t| − t·spread) and d2 = max(0, t − dmax, dmin − t) must satisfy d1² + d2² < radius².
- Passing light indices are written into the index texture at (x, y, offset + i). A tile column holds at most 2048 indices across all slices.
- The tile texture stores (offset << 12) | count for each froxel.

Studio also ships a four-pass variant behind a flag (Clear, AccumAndCreateMask, CountOffset, Gather; Vulkan shaders 0223 to 0227) that splits the same work into bitmask building and compaction. The single-pass version is enough for a port.

## Shading

Froxel lookup per pixel: p = FroxelTransform · worldPos; tile = floor(p.xy / p.w); slice = max(0, log2(p.w·0.1)·1.7095 + 1), where 1.7095 = 1/log2(1.5). Then fetch (offset << 12) | count and loop over the indices.

For each light:

1. Surface lights clamp the light position onto the rectangle: L = pos + axisU·clamp(dot(P − pos, axisU), −hu, hu) + axisV·clamp(dot(P − pos, axisV), −hv, hv) − P. Point and spot lights have zero extents, so L = pos − P.
2. Distance attenuation with r = clamp(|L|, start, range) − start and x = r / (range − start): `atten = (x²·(2x − 3) + 1) · 110 / (110 + r²)`.
3. Spot cone: s = clamp((dot(dir, −L̂) − spotCos) / (1 − spotCos), 0, 1); multiply by s² unless spotCos is 0.
4. Shadow (if `shadow ≥ 0`): 128 shadow slots, each with 6 face matrices (768 matrices in all) for cube-style point shadows. The face is picked by the dominant axis of the light-to-pixel vector, and the result is EVSM filtered.
5. Specular and diffuse use the same material terms as the sun, with the view vector from `CameraPosition`.

Sun shadows: four cascades picked by comparing the squared distance to `CascadeSphere0..3`, EVSM with `evsmPosExp` and `evsmNegExp`. Light bleeding is reduced with `clamp((p − 0.2)·1.25, 0, 1)` and the two moments take the minimum. The voxel grid's shadow term fades in past the shadow distance (`sunDist`, `hybridLerpSlope`).

Globals members the Future path adds over 2016 (names from the GLSL3 pack): `CascadeSphere0..3`, `localLightDist`, `sunDist`, `hybridLerpSlope`, `evsmPosExp`, `evsmNegExp`, `globalShadow`, `shadowBias`, `FroxelTransform`, `AmbientCube[12]`, `SkyGradientTop_EnvDiffuse`, `SkyGradientBottom_EnvSpec`, `AmbientColorNoIBL_CubeBlend`. Textures added: `PrefilteredEnvTexture`, `PrefilteredEnvIndoorTexture`, `PrefilteredEnvBlendTargetTexture`, `PrecomputedBRDFTexture`, `ShadowAtlasTexture`, `EvsmTexture0Texture`.

## What our renderer has today

`crates/rbx-render` implements the 2016 pipeline: the CPU voxel light grid (`lightgrid.rs`), a single blurred sun shadow map (`shadow.rs`) and Phong-style shading in `shaders/default.wgsl`. The voxel grid half of Future is already there.

## Porting plan

Each step runs on its own and keeps the 2016 look as the default until `Lighting.Technology` selects Future.

1. **Light records.** Gather PointLight, SpotLight and SurfaceLight instances each frame, keep the nearest 819, and write the 80-byte shading records and 48-byte view-space cull records plus 32-light chunk bounds, using the formulas above.
2. **Froxel culling in WGSL.** Port the single-pass cull as a compute shader: tile texture `r32sint` 3D at ceil(W/32) × ceil(H/32) × 33, index texture `r32sint` 3D with depth 2048. wgpu supports 3D storage textures on every backend we target, including WebGPU. Recreate both on resize.
3. **Forward light loop.** Add `FroxelTransform` to the globals and the froxel lookup plus light loop to `default.wgsl`, with the exact attenuation and spot terms. At this point local lights work without shadows.
4. **Sun EVSM cascades.** Replace the single shadow map with four cascades and EVSM moments, including the blur passes (the `ShadowEvsm*` shaders) and the light-bleed reduction.
5. **Local light shadows.** A shadow atlas with per-light cube faces and the 128-slot table. Allocation and caching live in `ShadowMapSystem` (`ShadowMapSystem::updatePerform` is referenced from `0x144903ed0`) and `ShadowObjectSpotNew` (`0x144b83660`). That code still needs decompiling.
6. **PBR environment lighting.** Prefiltered environment cubemaps (`EnvMapPBR`, `EnvMapView`), the precomputed BRDF lookup and the ambient cube. This is the largest remaining piece and needs its own decompilation pass.

Steps 1 to 3 are fully specified by the evidence above. Steps 4 to 6 have their shaders available and need more of the CPU side traced first.

## Reproducing the evidence

- Shader packs: `shaders.zip` in the build folder. The GLSL3 pack contains plain GLSL with struct names. The Vulkan pack contains SPIR-V modules that spirv-cross turns into GLSL with stripped names.
- Functions: imported into Ghidra 12.1.4 without auto-analysis. Function bounds come from the PE `.pdata` table, and single functions are decompiled with a headless script.
