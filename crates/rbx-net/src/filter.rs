use rbx_dom::{DataModel, InstanceId};
use rbx_reflection::{EventDescriptor, PropertyDescriptor, database};

const STARTER: &[&str] = &["StarterGui", "StarterPack", "StarterPlayer", "Teams"];
const STRICT_PROPERTIES: &[&str] = &["Jump", "Sit", "Throttle", "Steer", "SimulationRadius"];
const STRICT_EVENTS: &[&str] = &[
    "OnServerEvent",
    "PromptProductPurchaseFinished",
    "PromptPurchaseFinished",
    "PromptPurchaseRequested",
    "ClientPurchaseSuccess",
    "ServerPurchaseVerification",
    "Activated",
    "Deactivated",
    "SimulationRadiusChanged",
    "LuaDialogCallbackSignal",
    "ServerAdVerification",
    "ClientAdVerificationResults",
];
const REPARENTABLE: &[&str] = &["Tool", "Weld", "Accoutrement", "Seat", "SkateboardPlatform"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject,
    Disconnect,
}

pub struct Filter<'a> {
    pub dm: &'a DataModel,
    pub player: Option<InstanceId>,
    pub strict: bool,
}

fn under(dm: &DataModel, id: Option<InstanceId>, ancestor: InstanceId) -> bool {
    match id {
        Some(i) => i == ancestor || dm.is_descendant_of(i, ancestor),
        None => false,
    }
}

fn inside_class(dm: &DataModel, id: Option<InstanceId>, classes: &[&str]) -> bool {
    let mut cur = id;
    while let Some(i) = cur {
        if dm.class_of(i).is_some_and(|c| classes.contains(&c)) {
            return true;
        }
        cur = dm.parent_of(i);
    }
    false
}

fn is_a_any(class: &str, list: &[&str]) -> bool {
    list.iter().any(|c| database().is_a(class, c))
}

fn verdict(ok: bool) -> Verdict {
    if ok { Verdict::Accept } else { Verdict::Reject }
}

impl Filter<'_> {
    fn character(&self) -> Option<InstanceId> {
        self.player.and_then(|p| self.dm.get_ref(p, "Character"))
    }

    fn basic_parent(
        &self,
        id: Option<InstanceId>,
        class: &str,
        parent: Option<InstanceId>,
    ) -> Option<Verdict> {
        let dm = self.dm;
        if class == "Player" && id.is_none_or(|i| dm.parent_of(i).is_none()) {
            return Some(verdict(
                parent.is_some_and(|p| dm.class_of(p) == Some("Players")),
            ));
        }
        if inside_class(dm, parent, STARTER) || inside_class(dm, id, STARTER) {
            return Some(Verdict::Reject);
        }
        if let Some(i) = id
            && dm.instance(i).is_some_and(|inst| inst.parent_locked)
        {
            tracing::warn!("trying to set locked parent!");
            return Some(Verdict::Reject);
        }
        None
    }

    pub fn new_instance(&self, class: &str, parent: InstanceId) -> Verdict {
        let dm = self.dm;
        if database().is_a(class, "Message") {
            return Verdict::Reject;
        }
        if class == "Player" && self.player.is_some() {
            return Verdict::Disconnect;
        }
        if self.strict {
            if class == "Player" {
                return verdict(dm.class_of(parent) == Some("Players"));
            }
            if class == "StarterGear" {
                return Verdict::Accept;
            }
            if database().is_a(class, "Weld")
                && let Some(ch) = self.character()
            {
                return verdict(under(dm, Some(parent), ch));
            }
            return Verdict::Reject;
        }
        self.basic_parent(None, class, Some(parent))
            .unwrap_or(Verdict::Accept)
    }

    pub fn delete(&self, id: InstanceId) -> Verdict {
        let dm = self.dm;
        if self.strict {
            let ok = match (self.character(), self.player) {
                (Some(ch), _) => dm.is_descendant_of(id, ch),
                (None, Some(p)) => dm.is_descendant_of(id, p),
                _ => false,
            };
            return verdict(ok);
        }
        let class = dm.class_of(id).unwrap_or("Instance");
        self.basic_parent(Some(id), class, None)
            .unwrap_or(Verdict::Accept)
    }

    pub fn parent(&self, id: InstanceId, new_parent: Option<InstanceId>) -> Verdict {
        let dm = self.dm;
        let class = dm.class_of(id).unwrap_or("Instance");
        if !self.strict {
            return self
                .basic_parent(Some(id), class, new_parent)
                .unwrap_or(Verdict::Accept);
        }
        let old = dm.parent_of(id);
        let character = self.character();
        if let Some(ch) = character
            && under(dm, old, ch)
            && new_parent.is_none_or(|p| !under(dm, Some(p), ch))
        {
            return Verdict::Accept;
        }
        if let Some(p) = self.player
            && under(dm, old, p)
            && new_parent.is_none_or(|np| !under(dm, Some(np), p))
        {
            return Verdict::Accept;
        }
        if new_parent.is_none() || !is_a_any(class, REPARENTABLE) {
            return Verdict::Reject;
        }
        if let Some(ch) = character
            && under(dm, old, ch)
        {
            if database().is_a(class, "Weld")
                && dm.name_of(id) != "RightGrip"
                && dm
                    .get_ref(id, "Part0")
                    .is_some_and(|p0| dm.is_descendant_of(p0, ch))
            {
                return Verdict::Reject;
            }
            return Verdict::Accept;
        }
        if let Some(p) = self.player
            && under(dm, old, p)
        {
            return Verdict::Accept;
        }
        Verdict::Reject
    }

    pub fn property(&self, id: InstanceId, desc: &PropertyDescriptor) -> Verdict {
        let dm = self.dm;
        let class = dm.class_of(id).unwrap_or("Instance");
        if class == "Player" {
            return verdict(!matches!(desc.name.as_str(), "userId" | "UserId" | "Name"));
        }
        if database().is_a(class, "LuaSourceContainer")
            && matches!(desc.name.as_str(), "Source" | "LinkedSource")
        {
            return Verdict::Reject;
        }
        if self.strict {
            if desc.is_scriptable() || desc.can_xml_read() || desc.can_xml_write() {
                return verdict(STRICT_PROPERTIES.contains(&desc.name.as_str()));
            }
            return Verdict::Accept;
        }
        if inside_class(dm, Some(id), STARTER) {
            return Verdict::Reject;
        }
        if desc.name == "SimulationRadius" {
            return verdict(self.player == Some(id));
        }
        Verdict::Accept
    }

    pub fn event(&self, id: InstanceId, desc: &EventDescriptor) -> Verdict {
        let dm = self.dm;
        if dm.class_of(id) == Some("Player") && self.player != Some(id) {
            return Verdict::Reject;
        }
        if self.strict && !matches!(desc.security, rbx_reflection::Security::NotAccessible) {
            return verdict(STRICT_EVENTS.contains(&desc.name.as_str()));
        }
        Verdict::Accept
    }
}
