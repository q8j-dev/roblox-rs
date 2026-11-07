use crate::datamodel::DataModel;
use rbx_reflection::{InstanceId, Variant, database};
use std::collections::HashMap;
use std::sync::LazyLock;

pub enum SetOutcome {
    Unhandled,
    Handled,
    Replace(Variant),
    Error(String),
}

#[allow(unused_variables)]
pub trait Behavior: Sync + Send {
    fn aliases(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    fn computed(&self) -> &'static [&'static str] {
        &[]
    }

    fn init(&self, dm: &mut DataModel, id: InstanceId) {}

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        None
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        SetOutcome::Unhandled
    }

    fn changed(&self, dm: &mut DataModel, id: InstanceId, prop: &'static str) {}

    fn verify_set_parent(
        &self,
        dm: &DataModel,
        id: InstanceId,
        new_parent: Option<InstanceId>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn verify_add_child(
        &self,
        dm: &DataModel,
        id: InstanceId,
        child: InstanceId,
    ) -> Result<(), String> {
        Ok(())
    }

    fn verify_set_ancestor(
        &self,
        dm: &DataModel,
        id: InstanceId,
        new_parent: Option<InstanceId>,
        moving: InstanceId,
    ) -> Result<(), String> {
        Ok(())
    }

    fn ask_set_parent(&self, dm: &DataModel, id: InstanceId, parent: InstanceId) -> bool {
        false
    }

    fn ask_add_child(&self, dm: &DataModel, id: InstanceId, child: InstanceId) -> bool {
        false
    }

    fn ask_forbid_parent(&self, dm: &DataModel, id: InstanceId, parent: InstanceId) -> bool {
        false
    }

    fn ask_forbid_child(&self, dm: &DataModel, id: InstanceId, child: InstanceId) -> bool {
        false
    }

    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        child: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
    }

    fn child_added(&self, dm: &mut DataModel, id: InstanceId, child: InstanceId) {}

    fn child_removed(&self, dm: &mut DataModel, id: InstanceId, child: InstanceId) {}

    fn descendant_added(&self, dm: &mut DataModel, id: InstanceId, descendant: InstanceId) {}

    fn descendant_removing(&self, dm: &mut DataModel, id: InstanceId, descendant: InstanceId) {}

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {}
}

static REGISTRY: LazyLock<HashMap<&'static str, &'static dyn Behavior>> =
    LazyLock::new(crate::classes::registry);

static CHAINS: LazyLock<HashMap<&'static str, Vec<&'static dyn Behavior>>> = LazyLock::new(|| {
    let db = database();
    db.classes()
        .map(|c| {
            let chain: Vec<&'static dyn Behavior> = db
                .superclasses(&c.name)
                .filter_map(|s| REGISTRY.get(s.name.as_str()).copied())
                .collect();
            (c.name.as_str(), chain)
        })
        .collect()
});

pub fn behaviors_for(class: &str) -> &'static [&'static dyn Behavior] {
    CHAINS.get(class).map(|v| v.as_slice()).unwrap_or(&[])
}
