use crate::classes::terrain::TerrainData;
use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

#[derive(Clone)]
pub enum Change {
    Property {
        id: InstanceId,
        name: &'static str,
        old: Variant,
        new: Variant,
    },
    Name {
        id: InstanceId,
        old: String,
        new: String,
    },
    Parent {
        id: InstanceId,
        old: Option<InstanceId>,
        new: Option<InstanceId>,
    },
    Terrain {
        id: InstanceId,
        before: Box<TerrainData>,
        after: Box<TerrainData>,
    },
}

#[derive(Default)]
pub struct History {
    pub enabled: bool,
    pending: Vec<Change>,
    undo: Vec<(String, Vec<Change>)>,
    redo: Vec<(String, Vec<Change>)>,
    replaying: bool,
    terrain_base: HashMap<InstanceId, TerrainData>,
}

impl History {
    pub fn recording(&self) -> bool {
        self.enabled && !self.replaying
    }

    pub fn record(&mut self, c: Change) {
        if self.recording() {
            self.pending.push(c);
        }
    }
}

fn terrain_generation(t: &TerrainData) -> (u64, u64) {
    match (&t.legacy, &t.smooth) {
        (Some(g), _) => (1, g.changes.generation()),
        (_, Some(g)) => (2, g.changes.generation()),
        _ => (0, 0),
    }
}

impl DataModel {
    pub fn history_set_enabled(&mut self, enabled: bool) {
        if self.history.enabled == enabled {
            return;
        }
        self.history.enabled = enabled;
        if enabled {
            self.history.terrain_base = self
                .terrain_data
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect();
            self.history.pending.clear();
            self.history.undo.clear();
            self.history.redo.clear();
        }
    }

    fn capture_terrain(&mut self) {
        let ids: Vec<InstanceId> = self.terrain_data.keys().copied().collect();
        for id in ids {
            let now = &self.terrain_data[&id];
            let changed = match self.history.terrain_base.get(&id) {
                Some(base) => {
                    terrain_generation(base) != terrain_generation(now)
                        || base.is_smooth() != now.is_smooth()
                }
                None => now.non_empty_cell_count() > 0,
            };
            if changed {
                let before = self
                    .history
                    .terrain_base
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                let after = now.clone();
                self.history.terrain_base.insert(id, after.clone());
                self.history.pending.push(Change::Terrain {
                    id,
                    before: Box::new(before),
                    after: Box::new(after),
                });
            }
        }
    }

    pub fn history_waypoint(&mut self, name: &str) {
        if !self.history.enabled {
            return;
        }
        self.capture_terrain();
        if self.history.pending.is_empty() {
            return;
        }
        let changes = std::mem::take(&mut self.history.pending);
        self.history.undo.push((name.to_string(), changes));
        self.history.redo.clear();
    }

    pub fn history_reset(&mut self) {
        if !self.history.enabled {
            return;
        }
        self.history_waypoint("base");
        self.history.undo.clear();
        self.history.redo.clear();
    }

    pub fn history_can_undo(&self) -> Option<String> {
        if !self.history.enabled {
            return None;
        }
        self.history.undo.last().map(|(n, _)| n.clone())
    }

    pub fn history_can_redo(&self) -> Option<String> {
        if !self.history.enabled {
            return None;
        }
        self.history.redo.last().map(|(n, _)| n.clone())
    }

    fn apply_change(&mut self, c: &Change, forward: bool) {
        match c {
            Change::Property { id, name, old, new } => {
                self.set_unchecked(*id, name, if forward { new.clone() } else { old.clone() });
            }
            Change::Name { id, old, new } => self.set_name(*id, if forward { new } else { old }),
            Change::Parent { id, old, new } => {
                let _ = self.set_parent_ignore_lock(*id, if forward { *new } else { *old });
            }
            Change::Terrain { id, before, after } => {
                let mut data = if forward {
                    (**after).clone()
                } else {
                    (**before).clone()
                };
                data.dirty = true;
                if let Some(g) = data.legacy.as_mut() {
                    g.changes.reset();
                }
                if let Some(g) = data.smooth.as_mut() {
                    g.changes.reset();
                }
                self.history.terrain_base.insert(*id, data.clone());
                self.terrain_data.insert(*id, data);
            }
        }
    }

    pub fn history_undo(&mut self) -> Option<String> {
        if !self.history.enabled {
            return None;
        }
        self.capture_terrain();
        if !self.history.pending.is_empty() {
            let changes = std::mem::take(&mut self.history.pending);
            self.history.undo.push((String::new(), changes));
            self.history.redo.clear();
        }
        let (name, changes) = self.history.undo.pop()?;
        self.history.replaying = true;
        for c in changes.iter().rev() {
            self.apply_change(c, false);
        }
        self.history.replaying = false;
        self.history.redo.push((name.clone(), changes));
        Some(name)
    }

    pub fn history_redo(&mut self) -> Option<String> {
        if !self.history.enabled {
            return None;
        }
        let (name, changes) = self.history.redo.pop()?;
        self.history.replaying = true;
        for c in changes.iter() {
            self.apply_change(c, true);
        }
        self.history.replaying = false;
        self.history.undo.push((name.clone(), changes));
        Some(name)
    }
}
