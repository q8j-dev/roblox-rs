use crate::{DataModel, InstanceId, Variant};
use rbx_reflection::database;
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct Styles {
    pub props: HashMap<InstanceId, HashMap<String, Variant>>,
    pub pseudo: HashMap<(InstanceId, String), HashMap<String, Variant>>,
}

impl Styles {
    pub fn is_empty(&self) -> bool {
        self.props.is_empty() && self.pseudo.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
struct Compound {
    class: Option<String>,
    tags: Vec<String>,
    name: Option<String>,
    states: Vec<String>,
    queries: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Complex {
    parts: Vec<Compound>,
    pseudo: Option<String>,
}

struct Rule {
    selectors: Vec<Complex>,
    priority: i64,
    order: usize,
    sheet: InstanceId,
    rule: InstanceId,
}

fn parse_compound_into(c: &mut Compound, token: &str) {
    let mut rest = token;
    while !rest.is_empty() {
        let (kind, body) = if let Some(r) = rest.strip_prefix('.') {
            ('.', r)
        } else if let Some(r) = rest.strip_prefix('#') {
            ('#', r)
        } else if let Some(r) = rest.strip_prefix(':') {
            (':', r)
        } else if let Some(r) = rest.strip_prefix('@') {
            ('@', r)
        } else {
            ('c', rest)
        };
        let end = body.find(['.', '#', ':', '@']).unwrap_or(body.len());
        let word = &body[..end];
        rest = &body[end..];
        if word.is_empty() {
            continue;
        }
        match kind {
            '.' => c.tags.push(word.to_string()),
            '#' => c.name = Some(word.to_string()),
            ':' => c.states.push(word.to_string()),
            '@' => c.queries.push(word.to_string()),
            _ => c.class = Some(word.to_string()),
        }
    }
}

fn parse_complex(text: &str) -> Option<(bool, bool, Complex)> {
    let text = text.trim();
    let starts_child = text.starts_with('>');
    let starts_pseudo = text.starts_with("::");
    let mut complex = Complex::default();
    let mut current = Compound::default();
    let mut has_current = false;
    let spaced = text.replace('>', " > ");
    for token in spaced.split_whitespace() {
        if token == ">" {
            if has_current {
                complex.parts.push(std::mem::take(&mut current));
                has_current = false;
            }
            continue;
        }
        if let Some(p) = token.find("::") {
            let (before, after) = token.split_at(p);
            if !before.is_empty() {
                parse_compound_into(&mut current, before);
                has_current = true;
            }
            complex.pseudo = Some(after[2..].to_string());
            continue;
        }
        parse_compound_into(&mut current, token);
        has_current = true;
    }
    if has_current {
        complex.parts.push(current);
    }
    if complex.parts.is_empty() && complex.pseudo.is_none() {
        return None;
    }
    Some((starts_child, starts_pseudo, complex))
}

fn parse_selector(text: &str) -> Vec<(bool, bool, Complex)> {
    text.split(',').filter_map(parse_complex).collect()
}

fn combine(parent: &Complex, child: (bool, bool, Complex)) -> Complex {
    let (starts_child, starts_pseudo, child) = child;
    let mut out = parent.clone();
    if starts_pseudo {
        out.pseudo = child.pseudo;
        return out;
    }
    if starts_child || out.parts.is_empty() {
        out.parts.extend(child.parts);
    } else {
        let mut parts = child.parts.into_iter();
        if let (Some(last), Some(first)) = (out.parts.last_mut(), parts.next()) {
            if first.class.is_some() {
                last.class = first.class;
            }
            last.tags.extend(first.tags);
            if first.name.is_some() {
                last.name = first.name;
            }
            last.states.extend(first.states);
            last.queries.extend(first.queries);
        }
        out.parts.extend(parts);
    }
    if child.pseudo.is_some() {
        out.pseudo = child.pseudo;
    }
    out
}

fn sheet_derives(dm: &DataModel, sheet: InstanceId) -> Vec<InstanceId> {
    let mut list: Vec<(i64, usize, InstanceId)> = dm
        .children_of(sheet)
        .iter()
        .enumerate()
        .filter(|(_, c)| dm.class_of(**c) == Some("StyleDerive"))
        .filter_map(|(i, c)| match dm.get(*c, "StyleSheet") {
            Ok(Variant::Ref(Some(s))) => Some((number(dm, *c, "Priority") as i64, i, s)),
            _ => None,
        })
        .collect();
    list.sort_by_key(|(p, i, _)| (*p, *i));
    let mut out: Vec<InstanceId> = dm.style_derives(sheet).to_vec();
    out.extend(list.into_iter().map(|(_, _, s)| s));
    out
}

fn number(dm: &DataModel, id: InstanceId, name: &str) -> f64 {
    dm.get(id, name)
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn rule_children(dm: &DataModel, id: InstanceId) -> Vec<InstanceId> {
    dm.children_of(id)
        .iter()
        .copied()
        .filter(|c| dm.class_of(*c) == Some("StyleRule"))
        .collect()
}

fn collect_rules(
    dm: &DataModel,
    sheet: InstanceId,
    out: &mut Vec<Rule>,
    visited: &mut Vec<InstanceId>,
) {
    if visited.contains(&sheet) || visited.len() > 32 {
        return;
    }
    visited.push(sheet);
    for derived in sheet_derives(dm, sheet) {
        collect_rules(dm, derived, out, visited);
    }
    for rule in rule_children(dm, sheet) {
        collect_rule(dm, rule, sheet, None, out);
    }
}

fn collect_rule(
    dm: &DataModel,
    rule: InstanceId,
    sheet: InstanceId,
    parent: Option<&[Complex]>,
    out: &mut Vec<Rule>,
) {
    let text = dm
        .get(rule, "Selector")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let parsed = parse_selector(&text);
    let selectors: Vec<Complex> = match parent {
        None => parsed.into_iter().map(|(_, _, c)| c).collect(),
        Some(parents) => parents
            .iter()
            .flat_map(|p| parsed.iter().cloned().map(move |c| combine(p, c)))
            .collect(),
    };
    if selectors.is_empty() {
        return;
    }
    let order = out.len();
    out.push(Rule {
        selectors: selectors.clone(),
        priority: number(dm, rule, "Priority") as i64,
        order,
        sheet,
        rule,
    });
    for child in rule_children(dm, rule) {
        collect_rule(dm, child, sheet, Some(&selectors), out);
    }
}

fn state_name(dm: &DataModel, id: InstanceId) -> Option<&'static str> {
    let Ok(Variant::Enum(v)) = dm.get(id, "GuiState") else {
        return None;
    };
    let name = database().enum_desc("GuiState")?.name_of(v)?;
    Some(match name {
        "Idle" => "Idle",
        "Hover" => "Hover",
        "Press" => "Press",
        "NonInteractable" => "NonInteractable",
        _ => return None,
    })
}

fn matches_compound(dm: &DataModel, id: InstanceId, c: &Compound) -> bool {
    if let Some(class) = &c.class
        && !dm.is_a(id, class)
    {
        return false;
    }
    if let Some(name) = &c.name
        && dm.name_of(id) != name
    {
        return false;
    }
    if !c.queries.is_empty() {
        return false;
    }
    let tags = dm.tags(id);
    if !c.tags.iter().all(|t| tags.iter().any(|x| x == t)) {
        return false;
    }
    if !c.states.is_empty() {
        let state = state_name(dm, id);
        if !c
            .states
            .iter()
            .all(|s| state.is_some_and(|st| st.eq_ignore_ascii_case(s)))
        {
            return false;
        }
    }
    true
}

fn matches(dm: &DataModel, id: InstanceId, sel: &Complex) -> bool {
    let mut current = Some(id);
    for (i, part) in sel.parts.iter().rev().enumerate() {
        let Some(node) = current else {
            return false;
        };
        if !matches_compound(dm, node, part) {
            return false;
        }
        if i + 1 < sel.parts.len() {
            current = dm.parent_of(node);
        }
    }
    true
}

fn resolve_token(dm: &DataModel, sheet: InstanceId, value: &Variant) -> Option<Variant> {
    let mut value = value.clone();
    for _ in 0..16 {
        let name = match &value {
            Variant::String(s) => match s.strip_prefix('$') {
                Some(n) => n.to_string(),
                None => return Some(value),
            },
            _ => return Some(value),
        };
        value = find_token(dm, sheet, &name, &mut Vec::new())?;
    }
    None
}

fn find_token(
    dm: &DataModel,
    sheet: InstanceId,
    name: &str,
    visited: &mut Vec<InstanceId>,
) -> Option<Variant> {
    if visited.contains(&sheet) || visited.len() > 32 {
        return None;
    }
    visited.push(sheet);
    if let Some(v) = dm.attribute(sheet, name) {
        return Some(v.clone());
    }
    let derives = sheet_derives(dm, sheet);
    derives
        .into_iter()
        .rev()
        .find_map(|d| find_token(dm, d, name, visited))
}

fn coerce_for(class: &str, name: &str, value: Variant) -> Option<Variant> {
    let prop = database().find_property(class, name)?;
    match prop.ty.variant_type() {
        Some(t) => value.coerce(t),
        None => Some(value),
    }
}

fn link_roots(dm: &DataModel) -> Vec<(InstanceId, InstanceId)> {
    let root = dm.root();
    let mut out = Vec::new();
    for id in dm.descendants(root) {
        if dm.class_of(id) != Some("StyleLink") {
            continue;
        }
        let Ok(Variant::Ref(Some(sheet))) = dm.get(id, "StyleSheet") else {
            continue;
        };
        if let Some(parent) = dm.parent_of(id) {
            out.push((parent, sheet));
        }
    }
    out
}

pub fn compute(dm: &DataModel) -> Styles {
    let mut styles = Styles::default();
    let links = link_roots(dm);
    if links.is_empty() {
        return styles;
    }
    let linked: HashMap<InstanceId, InstanceId> = links.iter().copied().collect();
    for (scope, sheet) in links {
        let mut rules = Vec::new();
        collect_rules(dm, sheet, &mut rules, &mut Vec::new());
        if rules.is_empty() {
            continue;
        }
        rules.sort_by_key(|r| (r.priority, r.order));
        if std::env::var("STYLE_DEBUG").is_ok() {
            eprintln!(
                "style link scope={} rules={} first={:?}",
                dm.full_name(scope),
                rules.len(),
                rules
                    .first()
                    .map(|r| (&r.selectors, dm.style_properties(r.rule)))
            );
        }
        let mut stack = vec![scope];
        while let Some(node) = stack.pop() {
            for &child in dm.children_of(node) {
                if !linked.contains_key(&child) {
                    stack.push(child);
                }
            }
            let class = dm.class_of(node).unwrap_or("Instance");
            if !database().is_a(class, "GuiBase2d") && !is_component(class) {
                continue;
            }
            for rule in &rules {
                for sel in &rule.selectors {
                    if !matches(dm, node, sel) {
                        continue;
                    }
                    let target_class = sel.pseudo.as_deref().unwrap_or(class);
                    for (name, value) in dm.style_properties(rule.rule) {
                        let Some(v) = resolve_token(dm, rule.sheet, value)
                            .and_then(|v| coerce_for(target_class, name, v))
                        else {
                            continue;
                        };
                        let map = match &sel.pseudo {
                            Some(p) => styles.pseudo.entry((node, p.clone())).or_default(),
                            None => styles.props.entry(node).or_default(),
                        };
                        map.insert(name.clone(), v);
                    }
                }
            }
        }
    }
    styles
}

fn is_component(class: &str) -> bool {
    database().is_a(class, "UIComponent")
}

pub enum Component {
    Real(InstanceId),
    Phantom(String, HashMap<String, Variant>),
}

impl Component {
    pub fn get(&self, dm: &DataModel, name: &str) -> Variant {
        match self {
            Component::Real(id) => dm.effective(*id, name).unwrap_or(Variant::Nil),
            Component::Phantom(class, map) => map.get(name).cloned().unwrap_or_else(|| {
                database()
                    .find_property(class, name)
                    .map(|p| crate::defaults::default_value(class, p))
                    .unwrap_or(Variant::Nil)
            }),
        }
    }

    pub fn id(&self) -> Option<InstanceId> {
        match self {
            Component::Real(id) => Some(*id),
            Component::Phantom(..) => None,
        }
    }

    pub fn f32(&self, dm: &DataModel, name: &str) -> f32 {
        self.get(dm, name).as_f64().unwrap_or(0.0) as f32
    }

    pub fn enum_value(&self, dm: &DataModel, name: &str) -> u32 {
        match self.get(dm, name) {
            Variant::Enum(v) => v,
            v => v.as_f64().unwrap_or(0.0) as u32,
        }
    }

    pub fn boolean(&self, dm: &DataModel, name: &str, default: bool) -> bool {
        match self.get(dm, name) {
            Variant::Bool(b) => b,
            _ => default,
        }
    }
}

pub fn component(dm: &DataModel, id: InstanceId, class: &str) -> Option<Component> {
    let real = dm.children_of(id).iter().copied().find(|c| {
        dm.class_of(*c) == Some(class)
            && !matches!(dm.effective(*c, "Enabled"), Ok(Variant::Bool(false)))
    });
    if let Some(c) = real {
        return Some(Component::Real(c));
    }
    let map = dm.styled_component(id, class)?;
    if matches!(map.get("Enabled"), Some(Variant::Bool(false))) {
        return None;
    }
    Some(Component::Phantom(class.to_string(), map))
}
