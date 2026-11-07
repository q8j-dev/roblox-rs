use crate::behavior::behaviors_for;
use rbx_reflection::{PropertyDescriptor, ValueType, database};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub const CORE_PROPERTIES: &[&str] = &[
    "Name",
    "Parent",
    "Archivable",
    "archivable",
    "ClassName",
    "className",
    "RobloxLocked",
    "DataCost",
];

#[derive(Debug)]
pub struct ClassLayout {
    pub class: &'static str,
    pub slots: Vec<&'static PropertyDescriptor>,
    pub index: HashMap<&'static str, usize>,
    pub aliases: HashMap<&'static str, &'static str>,
    pub computed: HashSet<&'static str>,
    pub all: Vec<&'static PropertyDescriptor>,
}

impl ClassLayout {
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    pub fn canonical<'a>(&self, name: &'a str) -> &'a str {
        match self.aliases.get(name) {
            Some(target) => target,
            None => name,
        }
    }

    pub fn descriptor(&self, name: &str) -> Option<&'static PropertyDescriptor> {
        self.all.iter().find(|p| p.name == name).copied()
    }
}

fn same_type(a: &ValueType, b: &ValueType) -> bool {
    a == b
}

fn build(class: &'static str) -> ClassLayout {
    let db = database();
    let all: Vec<&'static PropertyDescriptor> = db.properties(class);
    let by_name: HashMap<&'static str, &'static PropertyDescriptor> =
        all.iter().map(|p| (p.name.as_str(), *p)).collect();
    let mut aliases: HashMap<&'static str, &'static str> = HashMap::new();
    let mut computed: HashSet<&'static str> = HashSet::new();
    for b in behaviors_for(class) {
        for (alias, target) in b.aliases() {
            if let (Some(a), Some(t)) = (by_name.get(alias), by_name.get(target)) {
                aliases.insert(a.name.as_str(), t.name.as_str());
            }
        }
        for name in b.computed() {
            if let Some(p) = by_name.get(name) {
                computed.insert(p.name.as_str());
            }
        }
    }
    for p in &all {
        let name = p.name.as_str();
        if CORE_PROPERTIES.contains(&name) || aliases.contains_key(name) || computed.contains(name)
        {
            continue;
        }
        if let Some(target) = p.deprecated_by.as_deref().filter(|t| !t.is_empty())
            && let Some(t) = by_name.get(target)
            && t.name != p.name
            && same_type(&t.ty, &p.ty)
        {
            aliases.insert(name, t.name.as_str());
            continue;
        }
    }
    for p in &all {
        let name = p.name.as_str();
        if CORE_PROPERTIES.contains(&name) || aliases.contains_key(name) || computed.contains(name)
        {
            continue;
        }
        if p.serialized() {
            continue;
        }
        let twin = all.iter().find(|q| {
            q.name != p.name
                && q.serialized()
                && q.name.eq_ignore_ascii_case(&p.name)
                && same_type(&q.ty, &p.ty)
                && !aliases.contains_key(q.name.as_str())
        });
        if let Some(t) = twin {
            aliases.insert(name, t.name.as_str());
        }
    }
    let resolve = |mut n: &'static str, aliases: &HashMap<&'static str, &'static str>| {
        let mut guard = 0;
        while let Some(t) = aliases.get(n) {
            if *t == n || guard > 8 {
                break;
            }
            n = t;
            guard += 1;
        }
        n
    };
    let keys: Vec<&'static str> = aliases.keys().copied().collect();
    for k in keys {
        let r = resolve(k, &aliases);
        aliases.insert(k, r);
    }
    for p in &all {
        if p.read_only && !aliases.contains_key(p.name.as_str()) {
            computed.insert(p.name.as_str());
        }
    }
    let mut slots = Vec::new();
    let mut index = HashMap::new();
    for p in &all {
        let name = p.name.as_str();
        if CORE_PROPERTIES.contains(&name) || aliases.contains_key(name) || computed.contains(name)
        {
            continue;
        }
        if p.ty.variant_type().is_none() {
            computed.insert(name);
            continue;
        }
        index.insert(name, slots.len());
        slots.push(*p);
    }
    for (alias, target) in &aliases {
        if let Some(&i) = index.get(target) {
            index.insert(alias, i);
        }
    }
    ClassLayout {
        class,
        slots,
        index,
        aliases,
        computed,
        all,
    }
}

static LAYOUTS: LazyLock<HashMap<&'static str, ClassLayout>> = LazyLock::new(|| {
    database()
        .classes()
        .map(|c| (c.name.as_str(), build(c.name.as_str())))
        .collect()
});

pub fn layout_for(class: &str) -> Option<&'static ClassLayout> {
    LAYOUTS.get(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_layout_aliases() {
        let l = layout_for("Part").expect("Part layout");
        assert_eq!(l.canonical("Size"), "size");
        assert_eq!(l.canonical("brickColor"), "BrickColor");
        assert!(l.slot("CFrame").is_some());
        assert!(l.slot("Size").is_some());
        assert_eq!(l.slot("Size"), l.slot("size"));
        assert!(l.computed.contains("Position"));
    }
}
