use rbx_reflection::{InstanceId, Variant};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TreeInstance {
    pub class: String,
    pub properties: Vec<(String, Variant)>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub service: bool,
    pub service_rooted: bool,
}

impl TreeInstance {
    pub fn new(class: impl Into<String>) -> TreeInstance {
        TreeInstance {
            class: class.into(),
            ..Default::default()
        }
    }

    pub fn get(&self, name: &str) -> Option<&Variant> {
        self.properties
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    pub fn set(&mut self, name: &str, value: Variant) {
        if let Some(slot) = self.properties.iter_mut().find(|(n, _)| n == name) {
            slot.1 = value;
        } else {
            self.properties.push((name.to_string(), value));
        }
    }

    pub fn name(&self) -> &str {
        self.get("Name").and_then(|v| v.as_str()).unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tree {
    pub instances: Vec<TreeInstance>,
    pub roots: Vec<usize>,
}

impl Tree {
    pub fn new() -> Tree {
        Tree::default()
    }

    pub fn id_of(index: usize) -> InstanceId {
        match InstanceId::from_raw(index as u64 + 1) {
            Some(id) => id,
            None => unreachable!("index + 1 is never zero"),
        }
    }

    pub fn index_of(id: InstanceId) -> usize {
        (id.raw() - 1) as usize
    }

    pub fn push(&mut self, instance: TreeInstance, parent: Option<usize>) -> usize {
        let index = self.instances.len();
        let mut instance = instance;
        instance.parent = parent;
        self.instances.push(instance);
        match parent {
            Some(p) => self.instances[p].children.push(index),
            None => self.roots.push(index),
        }
        index
    }

    pub fn preorder(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.instances.len());
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(i) = stack.pop() {
            out.push(i);
            for &c in self.instances[i].children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    pub fn postorder(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.instances.len());
        fn visit(tree: &Tree, i: usize, out: &mut Vec<usize>) {
            let mut stack = vec![(i, 0usize)];
            while let Some((node, child)) = stack.pop() {
                let children = &tree.instances[node].children;
                if child < children.len() {
                    stack.push((node, child + 1));
                    stack.push((children[child], 0));
                } else {
                    out.push(node);
                }
            }
        }
        for &r in &self.roots {
            visit(self, r, &mut out);
        }
        out
    }

    pub fn find_first_child(&self, parent: Option<usize>, name: &str) -> Option<usize> {
        let list = match parent {
            Some(p) => &self.instances[p].children,
            None => &self.roots,
        };
        list.iter()
            .copied()
            .find(|&c| self.instances[c].name() == name)
    }

    pub fn count_reachable(&self) -> usize {
        self.preorder().len()
    }
}
