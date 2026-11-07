use crate::gui::{GuiCmd, GuiItem, Rect, TextCmd, get_f32, get_i64};
use crate::world_gui::{GuiCamera, vec3};
use rbx_dom::{DataModel, InstanceId, Variant};
use rbx_math::{CFrame, Vec2, Vec3};

const DEFAULT_DISPLAY_DISTANCE: f32 = 100.0;
const HEALTH_COLORS: [[f32; 3]; 3] = [
    [1.0, 28.0 / 255.0, 0.0],
    [250.0 / 255.0, 235.0 / 255.0, 0.0],
    [27.0 / 255.0, 252.0 / 255.0, 107.0 / 255.0],
];

fn cframe(dm: &DataModel, id: InstanceId) -> Option<CFrame> {
    dm.get_raw(id, "CFrame").and_then(|v| v.as_cframe())
}

fn head_of(dm: &DataModel, humanoid: InstanceId) -> Option<InstanceId> {
    let model = dm.parent_of(humanoid)?;
    dm.find_first_child(model, "Head", false)
        .filter(|&h| dm.is_a(h, "BasePart"))
}

fn character_humanoid(dm: &DataModel, part: InstanceId) -> Option<InstanceId> {
    let mut p = dm.parent_of(part);
    while let Some(id) = p {
        if dm.is_a(id, "Model")
            && let Some(h) = dm.find_first_child_of_class(id, "Humanoid")
        {
            return Some(h);
        }
        p = dm.parent_of(id);
    }
    None
}

fn local_character(dm: &DataModel) -> Option<InstanceId> {
    dm.get_ref(dm.local_player?, "Character")
}

fn team_color(dm: &DataModel, humanoid: InstanceId) -> [f32; 3] {
    let white = [1.0, 1.0, 1.0];
    if dm.find_service("Teams").is_none() {
        return white;
    }
    let Some(players) = dm.find_service("Players") else {
        return white;
    };
    let model = dm.parent_of(humanoid);
    for &p in dm.children_of(players) {
        if dm.class_of(p) != Some("Player") || dm.get_bool(p, "Neutral") {
            continue;
        }
        let Some(character) = dm.get_ref(p, "Character") else {
            continue;
        };
        if Some(character) == model
            && dm.find_first_child_of_class(character, "Humanoid") == Some(humanoid)
        {
            if let Ok(Variant::BrickColor(b)) = dm.get(p, "TeamColor") {
                let c = b.color3();
                return [c.r, c.g, c.b];
            }
            return white;
        }
    }
    white
}

fn ray_box(origin: Vec3, dir: Vec3, cf: &CFrame, size: Vec3) -> Option<f32> {
    let o = cf.point_to_object_space(origin);
    let d = cf.vector_to_object_space(dir);
    let half = size * 0.5;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    for axis in 0..3 {
        let (oa, da, ha) = (o[axis], d[axis], half[axis]);
        if da.abs() < 1e-9 {
            if oa.abs() > ha {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((-ha - oa) / da, (ha - oa) / da);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

fn collect_blockers(dm: &DataModel, id: InstanceId, out: &mut Vec<(CFrame, Vec3)>) {
    for &c in dm.children_of(id) {
        if dm.is_a(c, "BasePart")
            && get_f32(dm, c, "Transparency") <= 0.99
            && character_humanoid(dm, c).is_none()
            && let Some(cf) = cframe(dm, c)
        {
            out.push((cf, vec3(dm, c, "Size")));
        }
        collect_blockers(dm, c, out);
    }
}

fn visible(cam: &GuiCamera, blockers: &[(CFrame, Vec3)], head: &(CFrame, Vec3)) -> bool {
    let origin = cam.cframe.translation;
    let dir = (head.0.translation - origin) * 2.0;
    let Some(t_head) = ray_box(origin, dir, &head.0, head.1) else {
        return false;
    };
    !blockers
        .iter()
        .any(|(cf, size)| ray_box(origin, dir, cf, *size).is_some_and(|t| t < t_head))
}

fn health_color(relative: f32) -> [f32; 3] {
    if relative <= 0.1 {
        return HEALTH_COLORS[0];
    }
    if relative >= 0.8 {
        return HEALTH_COLORS[2];
    }
    let samples = [0.1f32, 0.5, 0.8];
    let mut sum = [0.0f32; 3];
    let mut weights = 0.0f32;
    for (i, s) in samples.iter().enumerate() {
        let d = (relative - s).abs();
        if d <= 0.0 {
            return HEALTH_COLORS[i];
        }
        let w = 1.0 / (d * d);
        for (acc, c) in sum.iter_mut().zip(HEALTH_COLORS[i]) {
            *acc += w * c;
        }
        weights += w;
    }
    sum.map(|v| v / weights)
}

fn rect_item(r: Rect, color: [f32; 4]) -> GuiItem {
    GuiItem {
        cmd: GuiCmd::Rect { rect: r, color },
        clip: None,
        rotation: 0.0,
        pivot: Vec2::ZERO,
    }
}

fn rect(a: Vec2, b: Vec2) -> Rect {
    Rect {
        x0: a.x,
        y0: a.y,
        x1: b.x,
        y1: b.y,
    }
}

pub fn extract(
    dm: &DataModel,
    cam: &GuiCamera,
    humanoids: &[InstanceId],
    out: &mut Vec<(f32, Vec<GuiItem>)>,
) {
    let local_model = local_character(dm);
    let local_humanoid = local_model.and_then(|m| dm.find_first_child_of_class(m, "Humanoid"));
    let local_head = local_humanoid
        .and_then(|h| head_of(dm, h))
        .and_then(|h| cframe(dm, h));
    let mut blockers: Option<Vec<(CFrame, Vec3)>> = None;
    for &humanoid in humanoids {
        let model = dm.parent_of(humanoid);
        if model.is_some() && model == local_model {
            continue;
        }
        let display_type = get_i64(dm, humanoid, "DisplayDistanceType");
        if display_type == 2 {
            continue;
        }
        let mut occlude = false;
        let mut name_distance = DEFAULT_DISPLAY_DISTANCE;
        let mut health_distance = DEFAULT_DISPLAY_DISTANCE;
        if let Some(local) = local_humanoid {
            match get_i64(dm, local, "NameOcclusion") {
                2 => occlude = true,
                1 => {
                    occlude = dm.find_service("Teams").is_some()
                        && team_color(dm, humanoid) != team_color(dm, local)
                }
                _ => {}
            }
            let source = if display_type == 1 { humanoid } else { local };
            name_distance = get_f32(dm, source, "NameDisplayDistance");
            health_distance = get_f32(dm, source, "HealthDisplayDistance");
        }
        let Some(head) = head_of(dm, humanoid) else {
            continue;
        };
        let Some(head_cf) = cframe(dm, head) else {
            continue;
        };
        let above = head_cf.translation + Vec3::new(0.0, 1.5, 0.0);
        let distance = (cam.cframe.translation - above).length();
        if distance > 100.0 || get_f32(dm, head, "Transparency") > 0.99 {
            continue;
        }
        if occlude {
            let blockers = blockers.get_or_insert_with(|| {
                let mut b = Vec::new();
                if let Some(ws) = dm.find_service("Workspace") {
                    collect_blockers(dm, ws, &mut b);
                }
                b
            });
            if !visible(cam, blockers, &(head_cf, vec3(dm, head, "Size"))) {
                continue;
            }
        }
        let local_distance = local_head
            .map(|cf| (cf.translation - above).length())
            .unwrap_or(0.0);
        let font_size = if distance < 20.0 {
            24.0
        } else if distance < 50.0 {
            18.0
        } else {
            12.0
        };
        let Some((screen, _)) = cam.project(above) else {
            continue;
        };
        let fade = |limit: f32| {
            if local_distance > 0.9 * limit {
                ((limit - local_distance) / (0.1 * limit)).max(0.0)
            } else {
                1.0
            }
        };
        let name_alpha = fade(name_distance);
        let health_alpha = fade(health_distance);
        let mut items = Vec::new();
        let name = model.map(|m| dm.name_of(m).to_string()).unwrap_or_default();
        if name_distance >= local_distance && !name.is_empty() {
            let color = team_color(dm, humanoid);
            let y = 0.25 * color[0] + 0.21 * color[1] + 0.0722 * color[2];
            let lightness = (116.0 * y.cbrt() - 16.0) / 100.0;
            let stroke = if lightness >= 0.52 {
                [49.0 / 255.0, 49.0 / 255.0, 49.0 / 255.0, 0.5 * name_alpha]
            } else {
                [
                    227.0 / 255.0,
                    227.0 / 255.0,
                    227.0 / 255.0,
                    0.5 * name_alpha,
                ]
            };
            let pos = Vec2::new(screen.x, screen.y - 2.0);
            items.push(GuiItem {
                cmd: GuiCmd::Text(TextCmd {
                    text: name,
                    font: rbx_text::Font::SourceSans,
                    size: font_size,
                    scaled: false,
                    wrapped: false,
                    xalign: rbx_text::XAlign::Center,
                    yalign: rbx_text::YAlign::Bottom,
                    rect: rect(pos, pos),
                    color: [color[0], color[1], color[2], name_alpha],
                    stroke: Some(stroke),
                }),
                clip: None,
                rotation: 0.0,
                pivot: Vec2::ZERO,
            });
        }
        let max_health = get_f32(dm, humanoid, "MaxHealth");
        if max_health != 0.0 && health_distance >= local_distance {
            let relative = (get_f32(dm, humanoid, "Health") / max_health).clamp(0.0, 1.0);
            let width = font_size * 4.0;
            let (h, s) = (2.0f32, 2.0f32);
            let tl = Vec2::new(screen.x - 0.5 * width, screen.y + 2.0);
            let middle = tl.x + width * relative;
            let c = health_color(relative);
            let bar = [c[0], c[1], c[2], health_alpha];
            let v = Vec2::new;
            for (a, b) in [
                (v(0.0, -(s * 1.5)), v(width, -s)),
                (v(0.0, h + s), v(width, h + s * 1.5)),
                (v(-(s * 1.5), -s), v(0.0, h + s)),
                (v(width, -s), v(width + s * 1.5, h + s)),
            ] {
                items.push(rect_item(rect(tl + a, tl + b), bar));
            }
            let grey = [
                101.0 / 255.0,
                100.0 / 255.0,
                100.0 / 255.0,
                health_alpha * 0.5,
            ];
            let grey_opaque = [100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0, health_alpha];
            for (i, (a, b)) in [
                (v(0.0, -s), v(width, h + s)),
                (v(-s, -s / 2.0), v(0.0, h + s / 2.0)),
                (v(width, -s / 2.0), v(width + s, h + s / 2.0)),
            ]
            .into_iter()
            .enumerate()
            {
                let color = if i == 0 { grey } else { grey_opaque };
                items.push(rect_item(rect(tl + a, tl + b), color));
            }
            items.push(rect_item(rect(tl, v(middle, tl.y + h)), bar));
        }
        if !items.is_empty() {
            out.push((
                (head_cf.translation - cam.cframe.translation).length(),
                items,
            ));
        }
    }
}
