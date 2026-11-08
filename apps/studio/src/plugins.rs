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

fn plugin_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(groups) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut groups: Vec<PathBuf> = groups.filter_map(|e| e.ok().map(|e| e.path())).collect();
    groups.sort();
    for g in groups {
        if g.is_file() {
            if g.extension().is_some_and(|e| e == "lua") {
                out.push(g);
            }
            continue;
        }
        let Ok(files) = std::fs::read_dir(&g) else {
            continue;
        };
        let mut files: Vec<PathBuf> = files
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "lua"))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

impl Plugins {
    pub fn load(doc: &Document, dir: &Path) -> Plugins {
        let mut p = Plugins::default();
        for file in plugin_files(dir) {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    p.errors.push(format!("{name}: {e}"));
                    continue;
                }
            };
            match doc
                .runtime
                .run_plugin(&source, &name, file.parent().map(Path::to_path_buf))
            {
                Ok(()) => p.loaded.push(name),
                Err(e) => p.errors.push(format!("{name}: {e}")),
            }
        }
        p.refresh(doc);
        p
    }

    pub fn refresh(&mut self, doc: &Document) {
        self.buttons = doc.runtime.engine.lock().plugins.buttons.clone();
    }

    pub fn find(&self, label: &str) -> Option<InstanceId> {
        self.buttons
            .iter()
            .find(|b| b.text == label || b.tooltip == label)
            .map(|b| b.id)
    }

    pub fn click(&mut self, doc: &Document, button: InstanceId) {
        doc.runtime.click_plugin_button(button);
        self.refresh(doc);
    }

}

