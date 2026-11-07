use crate::runtime::engine;
use mlua::Lua;
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec3};

pub const COMMANDS: &[&str] = &[
    "SelectAll",
    "Copy",
    "Cut",
    "Paste",
    "PasteInto",
    "Delete",
    "Duplicate",
];

fn parts_of(dm: &DataModel, id: InstanceId, out: &mut Vec<InstanceId>) {
    if dm.is_a(id, "BasePart") {
        out.push(id);
    }
    for &c in dm.children_of(id) {
        parts_of(dm, c, out);
    }
}

fn bounds(dm: &DataModel, parts: &[InstanceId]) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for &p in parts {
        let Some(cf) = dm.get_raw(p, "CFrame").and_then(|v| v.as_cframe()) else {
            continue;
        };
        let size = dm
            .get(p, "Size")
            .ok()
            .and_then(|v| v.as_vec3())
            .unwrap_or(Vec3::ONE);
        let h = size * 0.5;
        for a in 0..8 {
            let corner = Vec3::new(
                if a & 4 != 0 { h.x } else { -h.x },
                if a & 2 != 0 { h.y } else { -h.y },
                if a & 1 != 0 { h.z } else { -h.z },
            );
            let w = cf.point_to_world_space(corner);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    lo.is_finite().then_some((lo, hi))
}

fn move_up_clear(dm: &mut DataModel, pasted: &[InstanceId], others: &[InstanceId]) {
    let mut parts = Vec::new();
    for &id in pasted {
        parts_of(dm, id, &mut parts);
    }
    let Some((lo, hi)) = bounds(dm, &parts) else {
        return;
    };
    let mut top = f32::NEG_INFINITY;
    let mut other_parts = Vec::new();
    for &o in others {
        parts_of(dm, o, &mut other_parts);
    }
    for p in other_parts {
        if let Some((olo, ohi)) = bounds(dm, &[p])
            && olo.x < hi.x
            && ohi.x > lo.x
            && olo.z < hi.z
            && ohi.z > lo.z
            && ohi.y > lo.y
        {
            top = top.max(ohi.y);
        }
    }
    if !top.is_finite() {
        return;
    }
    let lift = top - lo.y;
    for p in parts {
        if let Some(cf) = dm.get_raw(p, "CFrame").and_then(|v| v.as_cframe()) {
            let moved = CFrame::new(cf.rotation, cf.translation + Vec3::new(0.0, lift, 0.0));
            let _ = dm.set(p, "CFrame", Variant::CFrame(moved));
        }
    }
}

fn copy(lua: &Lua) {
    let selection = super::selection::current(lua);
    let eng = engine(lua);
    let mut e = eng.lock();
    for old in std::mem::take(&mut e.clipboard) {
        e.dm.release(old);
    }
    let mut copies = Vec::new();
    for id in selection {
        if let Ok(Some(c)) = e.dm.clone_instance(id) {
            e.dm.retain(c);
            copies.push(c);
        }
    }
    e.clipboard = copies;
}

fn delete(lua: &Lua) {
    let selection = super::selection::current(lua);
    {
        let eng = engine(lua);
        let mut e = eng.lock();
        for id in selection {
            if e.dm.contains(id) && e.dm.parent_of(id) != Some(e.dm.root()) {
                let _ = e.dm.set_parent(id, None);
            }
        }
    }
    super::selection::set(lua, Vec::new());
}

fn paste(lua: &Lua, into: bool) {
    let selection = super::selection::current(lua);
    let pasted = {
        let eng = engine(lua);
        let mut e = eng.lock();
        let Some(ws) = e.dm.find_service("Workspace") else {
            return;
        };
        let clipboard = e.clipboard.clone();
        if clipboard.is_empty() {
            return;
        }
        let decal = clipboard.len() == 1 && e.dm.is_a(clipboard[0], "Decal");
        let parent = if selection.len() != 1 || (decal && !into) {
            ws
        } else {
            let sel = selection[0];
            let grand = e.dm.parent_of(sel).and_then(|p| e.dm.parent_of(p));
            if into || e.dm.parent_of(sel).is_none() || grand.is_none() {
                sel
            } else {
                e.dm.parent_of(sel).unwrap_or(ws)
            }
        };
        let mut pasted = Vec::new();
        for id in clipboard {
            if let Ok(Some(c)) = e.dm.clone_instance(id)
                && e.dm.set_parent(c, Some(parent)).is_ok()
            {
                pasted.push(c);
            }
        }
        if selection.is_empty() && !into {
            let others: Vec<InstanceId> =
                e.dm.children_of(ws)
                    .iter()
                    .copied()
                    .filter(|c| !pasted.contains(c))
                    .collect();
            move_up_clear(&mut e.dm, &pasted, &others);
        }
        pasted
    };
    super::selection::set(lua, pasted);
}

fn select_all(lua: &Lua) {
    let items = {
        let eng = engine(lua);
        let e = eng.lock();
        let Some(ws) = e.dm.find_service("Workspace") else {
            return;
        };
        e.dm.children_of(ws)
            .iter()
            .copied()
            .filter(|&c| {
                (e.dm.is_a(c, "PVInstance") && !e.dm.is_a(c, "Terrain"))
                    && !e.dm.get_bool(c, "Locked")
            })
            .collect::<Vec<_>>()
    };
    super::selection::set(lua, items);
}

pub fn enabled(lua: &Lua, name: &str) -> Option<bool> {
    let selected = !super::selection::current(lua).is_empty();
    let clipboard = !engine(lua).lock().clipboard.is_empty();
    Some(match name {
        "SelectAll" => true,
        "Copy" | "Cut" | "Delete" | "Duplicate" => selected,
        "Paste" => clipboard,
        "PasteInto" => clipboard && super::selection::current(lua).len() == 1,
        _ => return None,
    })
}

pub fn run(lua: &Lua, name: &str) -> bool {
    match name {
        "SelectAll" => select_all(lua),
        "Copy" => copy(lua),
        "Cut" => {
            copy(lua);
            delete(lua);
        }
        "Paste" => paste(lua, false),
        "PasteInto" => paste(lua, true),
        "Delete" => delete(lua),
        "Duplicate" => {
            let selection = super::selection::current(lua);
            let saved = std::mem::take(&mut engine(lua).lock().clipboard);
            copy(lua);
            super::selection::set(lua, selection);
            paste(lua, false);
            let eng = engine(lua);
            let mut e = eng.lock();
            for c in std::mem::replace(&mut e.clipboard, saved) {
                e.dm.release(c);
            }
        }
        _ => return false,
    }
    true
}
