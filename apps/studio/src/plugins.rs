use rbx_dom::InstanceId;
use rbx_script::api::plugin::PluginButton;
use rbx_studio_ui::Document;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Plugins {
    pub loaded: Vec<String>,
    pub errors: Vec<String>,
    buttons: Vec<PluginButton>,
}

fn is_plugin_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "lua" | "rbxm" | "rbxmx"))
}

fn plugin_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for e in entries {
        if e.is_file() {
            if is_plugin_file(&e) {
                out.push(e);
            }
            continue;
        }
        let Ok(files) = std::fs::read_dir(&e) else {
            continue;
        };
        let mut files: Vec<PathBuf> = files
            .filter_map(|f| f.ok().map(|f| f.path()))
            .filter(|p| p.is_file() && is_plugin_file(p))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

impl Plugins {
    pub fn load(doc: &Document, builtin_dir: &Path, user_dir: &Path) -> Plugins {
        let mut p = Plugins::default();
        for (dir, builtin) in [(builtin_dir, true), (user_dir, false)] {
            for file in plugin_files(dir) {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let folder = file
                    .parent()
                    .map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf()));
                let result = if file.extension().is_some_and(|e| e == "lua") {
                    std::fs::read_to_string(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|src| doc.runtime.run_plugin(&src, &name, folder, builtin))
                } else {
                    std::fs::read(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|data| doc.runtime.run_plugin_model(&data, folder, builtin))
                };
                match result {
                    Ok(()) => p.loaded.push(name),
                    Err(e) => p.errors.push(format!("{name}: {e}")),
                }
            }
        }
        p.refresh(doc);
        p
    }

    pub fn refresh(&mut self, doc: &Document) {
        self.buttons = doc.runtime.engine.lock().plugins.buttons.clone();
    }

    pub fn find(&self, label: &str) -> Option<InstanceId> {
        self.proxy(label).map(|b| b.id)
    }

    pub fn proxy(&self, action: &str) -> Option<&PluginButton> {
        let mut parts = action.split('.');
        let text = parts.next().unwrap_or(action);
        let toolbar = parts.next().filter(|t| !t.is_empty());
        self.buttons
            .iter()
            .filter(|b| toolbar.is_none_or(|t| b.toolbar == t))
            .find(|b| b.text == text)
            .or_else(|| {
                self.buttons
                    .iter()
                    .find(|b| toolbar.is_none() && b.tooltip == text)
            })
    }

    pub fn click(&mut self, doc: &Document, button: InstanceId) {
        doc.runtime.click_plugin_button(button);
        self.refresh(doc);
    }
}
