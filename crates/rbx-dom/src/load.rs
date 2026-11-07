use crate::classes::is_service_class;
use crate::datamodel::{DataModel, DomError};
use rbx_format::{Tree, TreeInstance};
use rbx_reflection::{InstanceId, Variant, database};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions {
    pub merge_services: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SaveOptions {
    pub include_non_archivable: bool,
}

struct Created {
    id: InstanceId,
    tree_index: usize,
    existing_service: bool,
}

impl DataModel {
    pub fn load_place(&mut self, data: &[u8]) -> Result<Vec<String>, DomError> {
        let tree = rbx_format::read(data).map_err(|e| DomError::Message(e.to_string()))?;
        let root = self.root();
        let (_, warnings) = self.load_tree(
            &tree,
            Some(root),
            LoadOptions {
                merge_services: true,
            },
        )?;
        Ok(warnings)
    }

    pub fn load_model(&mut self, data: &[u8]) -> Result<(Vec<InstanceId>, Vec<String>), DomError> {
        let tree = rbx_format::read(data).map_err(|e| DomError::Message(e.to_string()))?;
        self.load_tree(&tree, None, LoadOptions::default())
    }

    pub fn load_tree(
        &mut self,
        tree: &Tree,
        parent: Option<InstanceId>,
        options: LoadOptions,
    ) -> Result<(Vec<InstanceId>, Vec<String>), DomError> {
        let mut warnings = Vec::new();
        let mut created: Vec<Created> = Vec::new();
        let mut map: HashMap<usize, InstanceId> = HashMap::new();
        let mut tops = Vec::new();
        let into_root = parent == Some(self.root());
        for &r in &tree.roots {
            self.load_create(
                tree,
                r,
                true,
                into_root && options.merge_services,
                &mut created,
                &mut map,
                &mut warnings,
            );
        }
        for c in &created {
            let inst = &tree.instances[c.tree_index];
            self.apply_properties(c.id, inst, &map, false, &mut warnings);
        }
        for &r in &tree.roots {
            self.load_parent(tree, r, parent, &map, &created, &mut warnings);
            if let Some(&id) = map.get(&r) {
                tops.push(id);
            }
        }
        for c in &created {
            let inst = &tree.instances[c.tree_index];
            self.apply_properties(c.id, inst, &map, true, &mut warnings);
        }
        Ok((tops, warnings))
    }

    #[allow(clippy::too_many_arguments)]
    fn load_create(
        &mut self,
        tree: &Tree,
        index: usize,
        top: bool,
        merge_services: bool,
        created: &mut Vec<Created>,
        map: &mut HashMap<usize, InstanceId>,
        warnings: &mut Vec<String>,
    ) {
        let inst = &tree.instances[index];
        let Some(desc) = database().class(&inst.class) else {
            warnings.push(format!(
                "Unknown object class \"{}\" while reading XML",
                inst.class
            ));
            return;
        };
        let class = desc.name.as_str();
        let (id, existing) =
            if top && merge_services && (inst.service_rooted || is_service_class(class)) {
                match self.find_service(class) {
                    Some(id) => (id, true),
                    None => match self.create_internal(class) {
                        Ok(id) => (id, false),
                        Err(e) => {
                            warnings.push(e.to_string());
                            return;
                        }
                    },
                }
            } else {
                match self.create_internal(class) {
                    Ok(id) => (id, false),
                    Err(e) => {
                        warnings.push(e.to_string());
                        return;
                    }
                }
            };
        map.insert(index, id);
        created.push(Created {
            id,
            tree_index: index,
            existing_service: existing,
        });
        for &c in &inst.children {
            self.load_create(tree, c, false, merge_services, created, map, warnings);
        }
    }

    fn load_parent(
        &mut self,
        tree: &Tree,
        index: usize,
        parent: Option<InstanceId>,
        map: &HashMap<usize, InstanceId>,
        created: &[Created],
        warnings: &mut Vec<String>,
    ) {
        let Some(&id) = map.get(&index) else { return };
        for &c in &tree.instances[index].children {
            self.load_parent(tree, c, Some(id), map, created, warnings);
        }
        let existing = created.iter().any(|c| c.id == id && c.existing_service);
        if existing {
            return;
        }
        if let Err(e) = self.set_parent(id, parent) {
            warnings.push(format!("{}: {e}", self.full_name(id)));
        }
    }

    fn apply_properties(
        &mut self,
        id: InstanceId,
        inst: &TreeInstance,
        map: &HashMap<usize, InstanceId>,
        refs: bool,
        warnings: &mut Vec<String>,
    ) {
        let Some(layout) = self.instance(id).map(|i| i.layout) else {
            return;
        };
        for (name, value) in &inst.properties {
            let is_ref = matches!(value, Variant::Ref(_));
            if is_ref != refs {
                continue;
            }
            match name.as_str() {
                "Name" => {
                    if let Some(s) = value.as_str() {
                        self.set_name(id, s);
                    }
                    continue;
                }
                "Archivable" | "archivable" => {
                    if let Some(b) = value.as_bool() {
                        let _ = self.set(id, "Archivable", Variant::Bool(b));
                    }
                    continue;
                }
                "Parent" | "ClassName" | "className" | "DataCost" => continue,
                _ => {}
            }
            let Some(desc) = layout.descriptor(name) else {
                continue;
            };
            if desc.read_only || !desc.can_xml_read() {
                continue;
            }
            let value = match value {
                Variant::Ref(r) => {
                    Variant::Ref(r.and_then(|t| map.get(&Tree::index_of(t)).copied()))
                }
                v => v.clone(),
            };
            if let Err(e) = self.set(id, name, value) {
                warnings.push(format!("{}.{name}: {e}", self.full_name(id)));
            }
        }
    }

    pub fn save_tree(&self, roots: &[InstanceId], options: SaveOptions) -> Tree {
        let mut tree = Tree::new();
        let mut map: HashMap<InstanceId, usize> = HashMap::new();
        let mut order: Vec<InstanceId> = Vec::new();
        for &r in roots {
            self.gather(r, None, &mut tree, &mut map, &mut order, options);
        }
        for &id in &order {
            let index = map[&id];
            let props = self.serialized_values(id, &map);
            tree.instances[index].properties = props;
        }
        tree
    }

    pub fn save_place_tree(&self) -> Tree {
        let roots: Vec<InstanceId> = self.children_of(self.root()).to_vec();
        self.save_tree(&roots, SaveOptions::default())
    }

    pub fn save_place_binary(&self) -> Result<Vec<u8>, DomError> {
        rbx_format::write_binary(
            &self.save_place_tree(),
            rbx_format::BinaryWriteOptions::default(),
        )
        .map_err(|e| DomError::Message(e.to_string()))
    }

    pub fn save_place_xml(&self) -> String {
        rbx_format::write_xml(&self.save_place_tree())
    }

    fn gather(
        &self,
        id: InstanceId,
        parent: Option<usize>,
        tree: &mut Tree,
        map: &mut HashMap<InstanceId, usize>,
        order: &mut Vec<InstanceId>,
        options: SaveOptions,
    ) {
        let Some(inst) = self.instance(id) else {
            return;
        };
        let serializable = database()
            .class(inst.class)
            .map(|c| c.functionality.is_serializable())
            .unwrap_or(false);
        if !serializable
            || (!inst.archivable && !options.include_non_archivable)
            || map.contains_key(&id)
        {
            return;
        }
        let mut t = TreeInstance::new(inst.class);
        t.service = is_service_class(inst.class);
        t.service_rooted = t.service && inst.parent == Some(self.root());
        let index = tree.push(t, parent);
        map.insert(id, index);
        order.push(id);
        for &c in &inst.children {
            self.gather(c, Some(index), tree, map, order, options);
        }
    }

    fn serialized_values(
        &self,
        id: InstanceId,
        map: &HashMap<InstanceId, usize>,
    ) -> Vec<(String, Variant)> {
        let Some(inst) = self.instance(id) else {
            return Vec::new();
        };
        let mut out = vec![("Name".to_string(), Variant::String(inst.name.clone()))];
        if let Some(p) = inst
            .layout
            .descriptor("archivable")
            .or_else(|| inst.layout.descriptor("Archivable"))
            && p.serialized()
        {
            out.push((p.name.clone(), Variant::Bool(inst.archivable)));
        }
        for (slot, p) in inst.layout.slots.iter().enumerate() {
            if !p.serialized() {
                continue;
            }
            let v = match &inst.values[slot] {
                Variant::Ref(Some(r)) => Variant::Ref(map.get(r).map(|&i| Tree::id_of(i))),
                v => v.clone(),
            };
            out.push((p.name.clone(), v));
        }
        out
    }
}
