use rbx_dom::{DataModel, InstanceId, database};

pub struct Container {
    pub class: &'static str,
    pub properties: bool,
    pub children: bool,
}

const fn c(class: &'static str, properties: bool, children: bool) -> Container {
    Container {
        class,
        properties,
        children,
    }
}

pub const CONTAINERS: &[Container] = &[
    c("ReplicatedFirst", false, true),
    c("Lighting", true, true),
    c("SoundService", true, false),
    c("StarterPack", false, true),
    c("StarterGui", false, true),
    c("StarterPlayer", false, true),
    c("CSGDictionaryService", false, true),
    c("Workspace", true, true),
    c("JointsService", false, true),
    c("Players", true, true),
    c("Teams", false, true),
    c("InsertService", false, true),
    c("Chat", false, true),
    c("FriendService", false, true),
    c("MarketplaceService", false, true),
    c("BadgeService", true, false),
    c("ReplicatedStorage", false, true),
    c("RobloxReplicatedStorage", false, true),
    c("TestService", true, true),
    c("LogService", true, false),
    c("PointsService", true, false),
    c("AdService", true, false),
];

pub fn container_of_class(class: &str) -> Option<&'static Container> {
    CONTAINERS.iter().find(|c| c.class == class)
}

pub fn container(dm: &DataModel, id: InstanceId) -> Option<&'static Container> {
    if dm.parent_of(id) != Some(dm.root()) {
        return None;
    }
    container_of_class(dm.class_of(id)?)
}

fn class_replicates(class: &str) -> (bool, bool) {
    match database().class(class) {
        Some(c) => (
            c.functionality.replicates(),
            c.functionality.replicates_to_player(),
        ),
        None => (false, false),
    }
}

pub fn player_of(dm: &DataModel, id: InstanceId) -> Option<InstanceId> {
    let mut cur = Some(id);
    while let Some(i) = cur {
        if dm.class_of(i) == Some("Player") {
            return Some(i);
        }
        cur = dm.parent_of(i);
    }
    None
}

pub fn in_scope(dm: &DataModel, id: InstanceId, viewer: Option<InstanceId>) -> bool {
    if !dm.contains(id) {
        return false;
    }
    let mut cur = id;
    let mut private_to: Option<InstanceId> = None;
    loop {
        let Some(class) = dm.class_of(cur) else {
            return false;
        };
        let Some(parent) = dm.parent_of(cur) else {
            return false;
        };
        if parent == dm.root() {
            return match container_of_class(class) {
                Some(c) => {
                    (cur == id || c.children) && private_to.is_none_or(|p| viewer == Some(p))
                }
                None => false,
            };
        }
        let (all, player) = class_replicates(class);
        if !all {
            if !player {
                return false;
            }
            let owner = player_of(dm, cur);
            if owner.is_none() {
                return false;
            }
            private_to = owner;
        }
        cur = parent;
    }
}
