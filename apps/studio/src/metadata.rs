use rbx_reflection::database;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
struct Entry {
    order: Option<i64>,
    image: Option<i64>,
    insertable: bool,
    preferred_parent: Option<String>,
}

#[derive(Default)]
pub struct ClassMetadata {
    entries: HashMap<String, Entry>,
}

impl ClassMetadata {
    pub fn load(path: &Path) -> ClassMetadata {
        let mut entries = HashMap::new();
        if let Some(Value::Object(map)) = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        {
            for (k, v) in map {
                entries.insert(
                    k,
                    Entry {
                        order: v.get("ExplorerOrder").and_then(Value::as_i64),
                        image: v.get("ExplorerImageIndex").and_then(Value::as_i64),
                        insertable: v.get("Insertable").and_then(Value::as_bool).unwrap_or(true),
                        preferred_parent: v
                            .get("PreferredParent")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    },
                );
            }
        }
        ClassMetadata { entries }
    }

    fn entry(&self, class: &str) -> Option<&Entry> {
        database()
            .superclasses(class)
            .find_map(|c| self.entries.get(&c.name))
    }

    pub fn order(&self, class: &str) -> i64 {
        match self.entry(class) {
            Some(e) => match e.order {
                Some(0) => 0,
                Some(o) => o,
                None => -1,
            },
            None => -1,
        }
    }

    pub fn image(&self, class: &str) -> usize {
        match self.entry(class).and_then(|e| e.image) {
            Some(i) if i > 0 => i as usize,
            _ => 0,
        }
    }

    pub fn insertable(&self, class: &str) -> bool {
        self.entries.get(class).is_none_or(|e| e.insertable)
    }

    pub fn preferred_parent(&self, class: &str) -> Option<&str> {
        self.entries.get(class)?.preferred_parent.as_deref()
    }
}
