use crate::behavior::{Behavior, SetOutcome};
use crate::datamodel::DataModel;
use rbx_math::{CFrame, Mat3, Vec3};
use rbx_reflection::{InstanceId, Variant};
use std::collections::HashMap;

const NOTHING: u8 = 0;
const HAS_HANDLE: u8 = 1;
const IN_WORKSPACE: u8 = 2;
const IN_CHARACTER: u8 = 3;
const HAS_TORSO: u8 = 4;
const EQUIPPED: u8 = 5;

const MOVE_OTHERS_DELAY: f64 = 0.2;

#[derive(Default, Clone)]
struct ToolEntry {
    state: u8,
    own_weld: bool,
    weld: Option<InstanceId>,
    mouse: Option<InstanceId>,
    equipped_mouse: Option<InstanceId>,
    touch: bool,
    character: Option<InstanceId>,
    torso: Option<InstanceId>,
    arm: Option<InstanceId>,
}

#[derive(Default)]
pub struct Tools {
    entries: HashMap<InstanceId, ToolEntry>,
    timers: Vec<(f64, InstanceId, InstanceId)>,
}

pub fn right_arm_grip() -> CFrame {
    CFrame::from_rows(
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0],
        Vec3::new(0.0, -1.0, 0.0),
    )
}

fn child_named_part(dm: &DataModel, parent: InstanceId, name: &str) -> Option<InstanceId> {
    dm.children_of(parent)
        .iter()
        .copied()
        .find(|c| dm.name_of(*c) == name && dm.is_a(*c, "BasePart"))
}

pub fn character_humanoid(dm: &DataModel, model: Option<InstanceId>) -> Option<InstanceId> {
    let m = model?;
    if !dm.is_a(m, "Model") {
        return None;
    }
    dm.children_of(m)
        .iter()
        .copied()
        .find(|c| dm.is_a(*c, "Humanoid"))
}

fn torso_of(dm: &DataModel, character: InstanceId) -> Option<InstanceId> {
    child_named_part(dm, character, "HumanoidRootPart")
        .or_else(|| child_named_part(dm, character, "Torso"))
}

fn visible_torso_of(dm: &DataModel, character: InstanceId) -> Option<InstanceId> {
    child_named_part(dm, character, "Torso")
}

fn right_arm_of(dm: &DataModel, character: InstanceId) -> Option<InstanceId> {
    child_named_part(dm, character, "Right Arm")
}

fn right_shoulder_of(dm: &DataModel, character: InstanceId) -> Option<InstanceId> {
    let torso = visible_torso_of(dm, character)?;
    dm.children_of(torso)
        .iter()
        .copied()
        .find(|c| dm.name_of(*c) == "Right Shoulder")
}

pub fn player_from_character(dm: &DataModel, character: InstanceId) -> Option<InstanceId> {
    let players = dm.find_service("Players")?;
    dm.children_of(players)
        .iter()
        .copied()
        .find(|p| dm.is_a(*p, "Player") && dm.get_ref(*p, "Character") == Some(character))
}

pub fn backpack_of(dm: &DataModel, player: InstanceId) -> Option<InstanceId> {
    dm.find_first_child_of_class(player, "Backpack")
}

fn has_provider(dm: &DataModel, parent: Option<InstanceId>) -> bool {
    parent.is_some_and(|p| dm.in_tree(p))
}

fn in_workspace(dm: &DataModel, id: InstanceId) -> bool {
    dm.find_service("Workspace")
        .is_some_and(|ws| dm.is_descendant_of(id, ws))
}

fn is_orthonormal(m: &Mat3) -> bool {
    let (x, y, z) = (m.x_axis, m.y_axis, m.z_axis);
    let near = |a: f32, b: f32| (a - b).abs() <= 1e-5;
    near(x.dot(y), 0.0)
        && near(y.dot(z), 0.0)
        && near(x.dot(z), 0.0)
        && near(x.length_squared(), 1.0)
        && near(y.length_squared(), 1.0)
        && near(z.length_squared(), 1.0)
}

fn clean_grip(mut c: CFrame) -> CFrame {
    let r = &mut c.rotation;
    if [r.x_axis, r.y_axis, r.z_axis]
        .iter()
        .any(|v| v.length_squared() < 1e-6)
    {
        r.x_axis = Vec3::new(0.0, -1.0, 0.0);
        r.y_axis = Vec3::new(1.0, 0.0, 0.0);
        r.z_axis = Vec3::new(0.0, 0.0, -1.0);
    }
    if !is_orthonormal(&c.rotation) {
        c.rotation = rbx_math::orthonormalize(&c.rotation);
    }
    c
}

fn safe_direction(v: Vec3) -> Vec3 {
    let m = v.length();
    if m > 1e-12 { v / m } else { Vec3::X }
}

impl DataModel {
    fn tool_entry(&mut self, tool: InstanceId) -> &mut ToolEntry {
        self.tools.entries.entry(tool).or_default()
    }

    fn tool_state(&self, tool: InstanceId) -> u8 {
        self.tools.entries.get(&tool).map_or(NOTHING, |e| e.state)
    }

    pub fn tool_handle(&self, tool: InstanceId) -> Option<InstanceId> {
        child_named_part(self, tool, "Handle")
    }

    pub fn tool_is_equipped(&self, tool: InstanceId) -> bool {
        self.tool_state(tool) >= EQUIPPED
    }

    pub fn tool_equipped_mouse(&self, tool: InstanceId) -> Option<InstanceId> {
        self.tools.entries.get(&tool).and_then(|e| e.equipped_mouse)
    }

    pub fn tool_grip(&self, tool: InstanceId) -> CFrame {
        self.get_raw(tool, "Grip")
            .and_then(|v| v.as_cframe())
            .unwrap_or_default()
    }

    fn tool_requires_handle(&self, tool: InstanceId) -> bool {
        self.get_raw(tool, "RequiresHandle")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    fn tool_desired_state(&self, tool: InstanceId) -> u8 {
        if self.tool_handle(tool).is_none() && self.tool_requires_handle(tool) {
            return NOTHING;
        }
        if !in_workspace(self, tool) {
            return NOTHING;
        }
        self.tool_desired_state_for(self.parent_of(tool))
    }

    fn tool_desired_state_for(&self, parent: Option<InstanceId>) -> u8 {
        if character_humanoid(self, parent).is_none() {
            return IN_WORKSPACE;
        }
        let Some(character) = parent else {
            return IN_WORKSPACE;
        };
        if torso_of(self, character).is_none() {
            return IN_CHARACTER;
        }
        if right_arm_of(self, character).is_none() || right_shoulder_of(self, character).is_none() {
            return HAS_TORSO;
        }
        EQUIPPED
    }

    fn tool_rebuild(&mut self, tool: InstanceId) {
        let desired = self.tool_desired_state(tool);
        self.tool_set_desired(tool, desired);
    }

    fn tool_set_desired(&mut self, tool: InstanceId, desired: u8) {
        let current = self.tool_state(tool);
        if desired == EQUIPPED && current == NOTHING {
            self.tool_set_backend_state(tool, EQUIPPED);
            if self.backend_processing {
                self.tool_up_to_in_character(tool);
                self.tool_up_to_has_torso(tool);
            }
            self.tool_up_to_equipped(tool);
            return;
        }
        if desired == NOTHING && current == EQUIPPED {
            self.tool_down_from_equipped(tool, false);
            self.tool_down_from_has_torso(tool);
            self.tool_down_from_in_character(tool);
            self.tool_set_backend_state(tool, NOTHING);
            return;
        }
        let mut state = current;
        while desired > state {
            match state {
                HAS_TORSO => self.tool_up_to_equipped(tool),
                IN_CHARACTER => self.tool_up_to_has_torso(tool),
                IN_WORKSPACE => self.tool_up_to_in_character(tool),
                NOTHING => self.tool_connect_touch(tool),
                _ => {}
            }
            state += 1;
        }
        while desired < state {
            match state {
                EQUIPPED => {
                    let backend = self.backend_processing;
                    self.tool_down_from_equipped(tool, backend)
                }
                HAS_TORSO => self.tool_down_from_has_torso(tool),
                IN_CHARACTER => self.tool_down_from_in_character(tool),
                HAS_HANDLE => self.tool_entry(tool).touch = false,
                _ => {}
            }
            state -= 1;
        }
        self.tool_set_backend_state(tool, state);
    }

    fn tool_set_backend_state(&mut self, tool: InstanceId, value: u8) {
        let current = self.tool_state(tool);
        if value == current {
            return;
        }
        if current < EQUIPPED && value >= EQUIPPED {
            let mouse = self.create_internal("Mouse").ok();
            if let Some(m) = mouse {
                self.retain(m);
            }
            if let Some(old) = self.tool_entry(tool).mouse {
                self.release(old);
            }
            let e = self.tool_entry(tool);
            e.mouse = mouse;
            e.equipped_mouse = mouse;
            self.fire_signal(tool, "Equipped", vec![Variant::Ref(mouse)]);
        }
        if current >= EQUIPPED && value < EQUIPPED {
            self.tool_entry(tool).equipped_mouse = None;
            self.fire_signal(tool, "Unequipped", Vec::new());
            if let Some(m) = self.tool_entry(tool).mouse
                && self.contains(m)
            {
                let _ = self.set(m, "Icon", Variant::String(String::new()));
            }
        }
        self.tool_entry(tool).state = value;
    }

    fn tool_connect_touch(&mut self, tool: InstanceId) {
        let has = self.tool_handle(tool).is_some();
        self.tool_entry(tool).touch = has;
    }

    fn tool_up_to_in_character(&mut self, tool: InstanceId) {
        let parent = self.parent_of(tool);
        self.tool_entry(tool).character = parent;
    }

    fn tool_down_from_in_character(&mut self, tool: InstanceId) {
        self.tool_entry(tool).character = None;
    }

    fn tool_up_to_has_torso(&mut self, tool: InstanceId) {
        let torso = self.parent_of(tool).and_then(|c| torso_of(self, c));
        self.tool_entry(tool).torso = torso;
    }

    fn tool_down_from_has_torso(&mut self, tool: InstanceId) {
        self.tool_entry(tool).torso = None;
    }

    fn tool_up_to_equipped(&mut self, tool: InstanceId) {
        self.tool_entry(tool).touch = false;
        let Some(character) = self.parent_of(tool) else {
            return;
        };
        if character_humanoid(self, Some(character)).is_none() {
            return;
        }
        let Some(arm) = right_arm_of(self, character) else {
            return;
        };
        let handle = self.tool_handle(tool);
        if self.tool_entry(tool).own_weld {
            if let Some(handle) = handle {
                let _ = self.set(handle, "CanCollide", Variant::Bool(false));
                let grip = self.tool_grip(tool);
                self.tool_build_weld(tool, arm, handle, right_arm_grip(), grip);
            }
        } else if self.backend_processing
            && let Some(handle) = handle
        {
            let existing = self
                .children_of(arm)
                .iter()
                .copied()
                .find(|c| self.name_of(*c) == "RightGrip" && self.is_a(*c, "Weld"));
            match existing {
                Some(w) => self.tool_entry(tool).weld = Some(w),
                None => self.tool_entry(tool).arm = Some(arm),
            }
            let _ = self.set(handle, "CanCollide", Variant::Bool(false));
        }
    }

    fn tool_build_weld(
        &mut self,
        tool: InstanceId,
        arm: InstanceId,
        handle: InstanceId,
        c0: CFrame,
        c1: CFrame,
    ) {
        if let Some(old) = self.tool_entry(tool).weld.take()
            && self.contains(old)
        {
            let _ = self.set_parent(old, None);
        }
        let Ok(weld) = self.create("Weld") else {
            return;
        };
        self.set_name(weld, "RightGrip");
        let _ = self.set(weld, "Part0", Variant::Ref(Some(arm)));
        let _ = self.set(weld, "Part1", Variant::Ref(Some(handle)));
        let _ = self.set(weld, "C0", Variant::CFrame(c0));
        let _ = self.set(weld, "C1", Variant::CFrame(c1));
        self.tool_entry(tool).weld = Some(weld);
        let _ = self.set_parent(weld, Some(arm));
    }

    fn tool_down_from_equipped(&mut self, tool: InstanceId, connect_touch: bool) {
        let mut old_character = None;
        let weld = self.tool_entry(tool).weld.take();
        self.tool_entry(tool).arm = None;
        if let Some(w) = weld
            && self.contains(w)
        {
            if let Some(arm) = self.get_ref(w, "Part0") {
                old_character = self.parent_of(arm).filter(|c| self.is_a(*c, "Model"));
            }
            let _ = self.set_parent(w, None);
        }
        if let Some(handle) = self.tool_handle(tool) {
            let _ = self.set(handle, "CanCollide", Variant::Bool(true));
            if let Some(character) = old_character
                && character_humanoid(self, Some(character)).is_some()
            {
                let cf = self.model_location(character);
                let mut pos = cf.translation;
                pos.y += 4.0;
                pos += cf.look_vector() * 8.0;
                let mut hcf = self
                    .get(handle, "CFrame")
                    .ok()
                    .and_then(|v| v.as_cframe())
                    .unwrap_or_default();
                hcf.translation = pos;
                let _ = self.set(handle, "CFrame", Variant::CFrame(hcf));
                let _ = self.set(
                    handle,
                    "RotVelocity",
                    Variant::Vector3(Vec3::new(1.0, 1.0, 1.0)),
                );
            }
        }
        if connect_touch {
            self.tool_connect_touch(tool);
        }
    }

    fn model_location(&self, model: InstanceId) -> CFrame {
        let primary = self
            .get_ref(model, "PrimaryPart")
            .filter(|p| self.is_descendant_of(*p, model))
            .or_else(|| {
                let mut best: Option<(InstanceId, f32)> = None;
                for d in self.descendants(model) {
                    if self.is_a(d, "BasePart") {
                        let s = self
                            .get(d, "Size")
                            .ok()
                            .and_then(|v| v.as_vec3())
                            .unwrap_or(Vec3::ONE);
                        let area = s.x * s.z;
                        if best.is_none_or(|(_, b)| area > b) {
                            best = Some((d, area));
                        }
                    }
                }
                best.map(|b| b.0)
            });
        primary
            .and_then(|p| self.get(p, "CFrame").ok())
            .and_then(|v| v.as_cframe())
            .unwrap_or_default()
    }

    pub(crate) fn tools_child_changed(
        &mut self,
        parent: InstanceId,
        child: InstanceId,
        added: bool,
    ) {
        if self.tools.entries.is_empty() {
            return;
        }
        let watching: Vec<InstanceId> = self
            .tools
            .entries
            .iter()
            .filter(|(t, e)| {
                **t != child
                    && (e.character == Some(parent)
                        || e.torso == Some(parent)
                        || (added && e.arm == Some(parent)))
            })
            .map(|(t, _)| *t)
            .collect();
        for tool in watching {
            if !self.contains(tool) {
                continue;
            }
            let e = self.tool_entry(tool).clone();
            if added && e.arm == Some(parent) {
                if self.is_a(child, "Weld")
                    && self.name_of(child) == "RightGrip"
                    && self.get_ref(child, "Part1") == self.tool_handle(tool)
                {
                    let entry = self.tool_entry(tool);
                    entry.weld = Some(child);
                    entry.arm = None;
                }
                continue;
            }
            if added {
                self.tool_rebuild(tool);
            } else if self.in_tree(tool) {
                if Some(child) == e.weld {
                    self.tool_set_desired(tool, HAS_TORSO);
                }
                self.tool_rebuild(tool);
            }
        }
    }

    pub fn tools_on_touched(&mut self, part: InstanceId, other: InstanceId) {
        let tool = match self.parent_of(part) {
            Some(t) if self.is_a(t, "Tool") => t,
            _ => return,
        };
        let Some(e) = self.tools.entries.get(&tool) else {
            return;
        };
        if !e.touch || e.state != IN_WORKSPACE || self.tool_handle(tool) != Some(part) {
            return;
        }
        let Some(character) = self.parent_of(other) else {
            return;
        };
        if self.tool_desired_state_for(Some(character)) != EQUIPPED {
            return;
        }
        let Some(player) = player_from_character(self, character) else {
            return;
        };
        self.move_all_tools_to_backpack(player);
        let _ = self.set_parent(tool, Some(character));
        let due = self.game_time + MOVE_OTHERS_DELAY;
        self.tools.timers.push((due, tool, player));
    }

    pub fn move_all_tools_to_backpack(&mut self, player: InstanceId) {
        let Some(character) = self.get_ref(player, "Character") else {
            return;
        };
        let backpack = backpack_of(self, player);
        while let Some(tool) = self.find_first_child_of_class(character, "Tool") {
            if self.set_parent(tool, backpack).is_err() || backpack.is_none() {
                break;
            }
        }
    }

    pub fn tools_step(&mut self) {
        if self.tools.timers.is_empty() {
            return;
        }
        let now = self.game_time;
        let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tools.timers)
            .into_iter()
            .partition(|(t, _, _)| *t <= now);
        self.tools.timers = rest;
        for (_, tool, player) in due {
            self.move_other_tools_to_backpack(tool, player);
        }
    }

    fn move_other_tools_to_backpack(&mut self, keep: InstanceId, player: InstanceId) {
        if !self.contains(keep) || !self.contains(player) || self.tool_state(keep) < EQUIPPED {
            return;
        }
        let Some(backpack) = backpack_of(self, player) else {
            return;
        };
        let Some(character) = self.get_ref(player, "Character") else {
            return;
        };
        let others: Vec<InstanceId> = self
            .children_of(character)
            .iter()
            .copied()
            .filter(|c| *c != keep && self.is_a(*c, "Tool"))
            .collect();
        for t in others {
            let _ = self.set_parent(t, Some(backpack));
        }
        if let Some(bin) = self.find_first_child_of_class(backpack, "HopperBin") {
            let _ = self.set(bin, "Active", Variant::Bool(false));
        }
    }

    pub fn humanoid_unequip_tools(&mut self, humanoid: InstanceId) -> Result<(), String> {
        let Some(character) = self.parent_of(humanoid).filter(|m| self.is_a(*m, "Model")) else {
            return Ok(());
        };
        let Some(player) = player_from_character(self, character) else {
            return Err(
                "UnequipTools can only be called on a Humanoid with a corresponding Player object."
                    .into(),
            );
        };
        let Some(backpack) = backpack_of(self, player) else {
            return Ok(());
        };
        let tools: Vec<InstanceId> = self
            .children_of(character)
            .iter()
            .copied()
            .filter(|c| self.is_a(*c, "Tool"))
            .collect();
        for t in tools {
            let _ = self.set_parent(t, Some(backpack));
        }
        Ok(())
    }

    pub fn humanoid_equip_tool(
        &mut self,
        humanoid: InstanceId,
        tool: InstanceId,
    ) -> Option<String> {
        if !self.is_a(tool, "Tool") {
            return None;
        }
        let character = self
            .parent_of(humanoid)
            .filter(|m| self.is_a(*m, "Model"))?;
        let warning = self.humanoid_unequip_tools(humanoid).err();
        let _ = self.set_parent(tool, Some(character));
        warning
    }

    fn tool_ancestry_changed(
        &mut self,
        tool: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        if has_provider(self, old_parent) {
            self.tool_entry(tool).own_weld = false;
            if self.backend_processing {
                self.tool_set_desired(tool, NOTHING);
            } else {
                self.tool_set_backend_state(tool, NOTHING);
            }
        }
        if !has_provider(self, new_parent) {
            if self
                .tools
                .entries
                .get(&tool)
                .is_some_and(|e| e.state == NOTHING && e.weld.is_none())
                && let Some(m) = self.tools.entries.remove(&tool).and_then(|e| e.mouse)
            {
                self.release(m);
            }
            return;
        }
        let parent = self.parent_of(tool);
        if self.backend_processing {
            let mut player = None;
            if self.frontend_processing {
                self.tool_entry(tool).own_weld = true;
            } else if let Some(character) = parent
                && character_humanoid(self, Some(character)).is_some()
            {
                player = player_from_character(self, character);
                if player.is_none() {
                    self.tool_entry(tool).own_weld = true;
                }
            }
            if let Some(p) = player
                && old_parent != backpack_of(self, p)
            {
                self.tool_entry(tool).own_weld = true;
            }
            self.tool_rebuild(tool);
        } else if let Some(local) = self.local_player
            && parent.is_some()
            && parent == self.get_ref(local, "Character")
        {
            if old_parent == backpack_of(self, local) {
                self.tool_entry(tool).own_weld = true;
            }
            self.tool_rebuild(tool);
        }
    }

    pub fn tool_activate(&mut self, tool: InstanceId) -> bool {
        if self.tool_state(tool) != EQUIPPED {
            return false;
        }
        self.fire_signal(tool, "Activated", Vec::new());
        true
    }

    pub fn tool_deactivate(&mut self, tool: InstanceId) {
        self.fire_signal(tool, "Deactivated", Vec::new());
    }
}

pub struct ToolBehavior;

impl Behavior for ToolBehavior {
    fn computed(&self) -> &'static [&'static str] {
        &["GripPos", "GripForward", "GripUp", "GripRight"]
    }

    fn get(&self, dm: &DataModel, id: InstanceId, prop: &str) -> Option<Variant> {
        let grip = dm.tool_grip(id);
        match prop {
            "GripPos" => Some(Variant::Vector3(grip.translation)),
            "GripForward" => Some(Variant::Vector3(grip.look_vector())),
            "GripUp" => Some(Variant::Vector3(grip.up_vector())),
            "GripRight" => Some(Variant::Vector3(grip.right_vector())),
            _ => None,
        }
    }

    fn set(&self, dm: &mut DataModel, id: InstanceId, prop: &str, value: &Variant) -> SetOutcome {
        let grip = dm.tool_grip(id);
        let v = || value.as_vec3();
        let next = match prop {
            "Grip" => {
                let Some(c) = value.as_cframe() else {
                    return SetOutcome::Unhandled;
                };
                let c = clean_grip(c);
                if let Some(w) = dm.tools.entries.get(&id).and_then(|e| e.weld)
                    && dm.contains(w)
                {
                    let _ = dm.set(w, "C1", Variant::CFrame(c));
                }
                return SetOutcome::Replace(Variant::CFrame(c));
            }
            "GripPos" => {
                let Some(p) = v() else {
                    return SetOutcome::Unhandled;
                };
                CFrame::new(grip.rotation, p)
            }
            "GripForward" => {
                let Some(f) = v() else {
                    return SetOutcome::Unhandled;
                };
                let old_y = grip.up_vector();
                let z = -safe_direction(f);
                let x = old_y.cross(z).normalize_or_zero();
                let y = z.cross(x).normalize_or_zero();
                CFrame::new(Mat3::from_cols(x, y, z), grip.translation)
            }
            "GripUp" => {
                let Some(u) = v() else {
                    return SetOutcome::Unhandled;
                };
                let old_x = grip.right_vector();
                let y = safe_direction(u);
                let z = old_x.cross(y).normalize_or_zero();
                let x = y.cross(z).normalize_or_zero();
                CFrame::new(Mat3::from_cols(x, y, z), grip.translation)
            }
            "GripRight" => {
                let Some(r) = v() else {
                    return SetOutcome::Unhandled;
                };
                let old_y = grip.up_vector();
                let x = safe_direction(r);
                let z = x.cross(old_y).normalize_or_zero();
                let y = z.cross(x).normalize_or_zero();
                CFrame::new(Mat3::from_cols(x, y, z), grip.translation)
            }
            _ => return SetOutcome::Unhandled,
        };
        match dm.set(id, "Grip", Variant::CFrame(next)) {
            Ok(()) => SetOutcome::Handled,
            Err(e) => SetOutcome::Error(e.to_string()),
        }
    }

    fn child_added(&self, dm: &mut DataModel, id: InstanceId, _child: InstanceId) {
        if dm.in_tree(id) && dm.backend_processing {
            dm.tool_rebuild(id);
        }
    }

    fn child_removed(&self, dm: &mut DataModel, id: InstanceId, _child: InstanceId) {
        if dm.in_tree(id) && dm.backend_processing {
            dm.tool_rebuild(id);
        }
    }

    fn ancestry_changed(
        &self,
        dm: &mut DataModel,
        id: InstanceId,
        _child: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    ) {
        dm.tool_ancestry_changed(id, old_parent, new_parent);
    }

    fn destroying(&self, dm: &mut DataModel, id: InstanceId) {
        if dm.tools.entries.contains_key(&id) {
            dm.tool_set_desired(id, NOTHING);
        }
        if let Some(m) = dm.tools.entries.remove(&id).and_then(|e| e.mouse) {
            dm.release(m);
        }
        dm.tools.timers.retain(|(_, t, _)| *t != id);
    }
}

pub fn register(map: &mut HashMap<&'static str, &'static dyn Behavior>) {
    map.insert("Tool", &ToolBehavior);
}
