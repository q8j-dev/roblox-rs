# InExperience.rbxm inventory

Source: `extracontent-models.zip` → `InExperience/InExperience.rbxm` from the Windows client
`version-f0ea5da9412f4e35`, copied unchanged to `assets/inexperience/` with its
`InExperience_checksum` (128 hex digits, not a plain SHA-512 of the file, kept as shipped).

## File format

- Binary model, version 0. Chunks are zstd compressed, not LZ4.
- Property types past the 2016 set: SecurityCapabilities (`Capabilities` on every instance),
  Int64 (`SourceAssetId`), and SharedString (`Source`).
- 38,258 instances: 1 root Folder `PatchRoot` with `CoreScripts` and `DataModelInstances`
  (`CorePackages`, `CoreGui`), 2,794 Folders, 35,456 ModuleScripts, 2 LocalizationTables,
  6 StringValues. Full listing: `tree.txt`, where `/` in names is shown as `_`
  (the CoreScripts are named like `CoreScripts/ExperienceChatMain`).

## Scripts

Every ModuleScript `Source` is compiled Luau, never text:

- An 8-byte prefix `52 7a 10 4a 5b 32 09 e0`, identical on all 35,456 modules.
- Then a standard Luau bytecode blob: version 9, type info version 3. Version 9 is listed as
  supported in our vendored Luau (`Common/include/Luau/Bytecode.h`, luau 0.745).
- Opcode bytes are stored multiplied by 227 mod 256. Multiplying by 203, the inverse, gives
  opcodes that line up with `LuauOpcode`; AUX words are not encoded. Evidence: decoding
  `CorePackages/Workspace/Packages/PlatformLeaderboardsRcc` gives PREPVARARGS, GETIMPORT,
  GETTABLEKS, CALL, RETURN with consistent operands.

`crates/luau-bytecode` parses this format from the loader in `VM/src/lvmload.cpp`, decodes the
opcodes, and writes plain bytecode back. All 35,456 modules parse, and every one round-trips
through write and parse to an identical module.

No plain-text copy of these scripts ships in the client: `extracontent-luapackages.zip` holds
only Foundation sprite sheets and BuilderIcons fonts, and there is no `extracontent-scripts.zip`.

## Entry point

`PatchRoot/CoreScripts/StarterScript` is the client entry. It requires the topbar
(`InExperienceTopBar`, `TopBar`, `Chrome`), chat (`CoreScripts/ExperienceChatMain`), the
settings hub (`Settings`), and about 60 other CoreScripts, each gated by fast flags,
`GetEngineFeature`, IXP experiment layers or policy checks.

## Lists

- `globals.txt`: every global and import path read (GETIMPORT, GETGLOBAL), with counts.
- `methods.txt`: every NAMECALL method name, with counts.
- `backport-list.md`: the 2026 API dump diffed against our 2016 database, limited to names the
  bytecode references. This covers the whole package; what the topbar, chat and menu need at
  run time is a subset.

Regenerate with:

```
cargo run -p rbx-format --example unpack -- assets/inexperience/InExperience.rbxm <dir>
cargo run -p luau-bytecode --example inventory -- assets/inexperience/InExperience.rbxm <dir> [path filter for .dis files]
python3 docs/inexperience/api_diff.py crates/rbx-reflection/data/api.json <Full-API-Dump.json> <dir> docs/inexperience/backport-list.md
```

The API dump comes from `setup.rbxcdn.com/version-67c8b1dedbb74fed-Full-API-Dump.json`, the
Studio build of the same release. `version-f0ea5da9412f4e35-Full-API-Dump.json` returns 403,
because dumps are only published for Studio builds.
