use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlagKind {
    Flag,
    Int,
    String,
}

impl FlagKind {
    fn prefix(self) -> &'static str {
        match self {
            FlagKind::Flag => "FFlag",
            FlagKind::Int => "FInt",
            FlagKind::String => "FString",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FlagValue {
    Bool(bool),
    Int(i64),
    String(String),
}

impl FlagValue {
    fn parse(kind: FlagKind, raw: &serde_json::Value) -> Option<FlagValue> {
        Some(match (kind, raw) {
            (FlagKind::Flag, serde_json::Value::Bool(b)) => FlagValue::Bool(*b),
            (FlagKind::Flag, serde_json::Value::String(s)) => {
                FlagValue::Bool(s.eq_ignore_ascii_case("true"))
            }
            (FlagKind::Int, serde_json::Value::Number(n)) => FlagValue::Int(n.as_i64()?),
            (FlagKind::Int, serde_json::Value::String(s)) => FlagValue::Int(s.trim().parse().ok()?),
            (FlagKind::String, serde_json::Value::String(s)) => FlagValue::String(s.clone()),
            (FlagKind::String, other) => FlagValue::String(other.to_string()),
            _ => return None,
        })
    }

    fn zero(kind: FlagKind) -> FlagValue {
        match kind {
            FlagKind::Flag => FlagValue::Bool(false),
            FlagKind::Int => FlagValue::Int(0),
            FlagKind::String => FlagValue::String(String::new()),
        }
    }

    fn describe(&self) -> String {
        match self {
            FlagValue::Bool(b) => b.to_string(),
            FlagValue::Int(i) => i.to_string(),
            FlagValue::String(s) => format!("{s:?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FlagRead {
    pub default: Option<String>,
    pub value: String,
}

#[derive(Default)]
pub struct FlagStore {
    defined: BTreeMap<(FlagKind, String), FlagValue>,
    overrides: BTreeMap<String, serde_json::Value>,
    baseline: BTreeMap<String, serde_json::Value>,
    engine_defaults: BTreeMap<String, serde_json::Value>,
    engine_features: BTreeMap<String, bool>,
    feature_defaults: BTreeMap<String, bool>,
    ixp_values: BTreeMap<String, serde_json::Value>,
    ixp: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    versions: BTreeMap<String, i64>,
    pub reads: BTreeMap<String, FlagRead>,
}

fn bare(name: &str) -> &str {
    for p in [
        "DFFlag", "SFFlag", "FFlag", "DFInt", "SFInt", "FInt", "DFString", "SFString", "FString",
    ] {
        if let Some(rest) = name.strip_prefix(p)
            && rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            return rest;
        }
    }
    name
}

impl FlagStore {
    pub fn load_json(&mut self, text: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(map) = value else {
            return Err("flag settings must be a JSON object".into());
        };
        for (key, v) in map {
            match (key.as_str(), v) {
                ("EngineFeatures", serde_json::Value::Object(features)) => {
                    for (name, on) in features {
                        self.engine_features
                            .insert(name, on.as_bool().unwrap_or(false));
                    }
                }
                ("IXP", serde_json::Value::Object(layers)) => {
                    for (layer, vars) in layers {
                        if let serde_json::Value::Object(vars) = vars {
                            self.ixp.insert(layer, vars);
                        }
                    }
                }
                (_, v) => {
                    self.overrides.insert(bare(&key).to_string(), v);
                }
            }
        }
        Ok(())
    }

    pub fn load_baseline_json(&mut self, text: &str) -> Result<usize, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(map) = value else {
            return Err("client settings must be a JSON object".into());
        };
        let count = map.len();
        for (key, v) in map {
            self.baseline.insert(bare(&key).to_string(), v);
        }
        Ok(count)
    }

    pub fn load_engine_features_json(&mut self, text: &str) -> Result<usize, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(map) = value else {
            return Err("engine features must be a JSON object".into());
        };
        let count = map.len();
        for (name, on) in map {
            if let Some(on) = on.as_bool() {
                self.feature_defaults.insert(name, on);
            }
        }
        Ok(count)
    }

    pub fn load_engine_defaults_json(&mut self, text: &str) -> Result<usize, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(map) = value else {
            return Err("engine flag defaults must be a JSON object".into());
        };
        let count = map.len();
        self.engine_defaults.extend(map);
        Ok(count)
    }

    fn raw(&self, name: &str) -> Option<&serde_json::Value> {
        self.overrides
            .get(name)
            .or_else(|| self.ixp_values.get(name))
            .or_else(|| self.baseline.get(name))
    }

    pub fn load_ixp_values_json(&mut self, text: &str) -> Result<usize, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(map) = value else {
            return Err("ixp settings must be a JSON object".into());
        };
        let mut count = 0;
        for (key, v) in map {
            let (Some(flag), Some(text)) = (key.strip_suffix("_IXPValue"), v.as_str()) else {
                continue;
            };
            let assigned = text.split(';').next().unwrap_or("").to_string();
            self.ixp_values
                .insert(bare(flag).to_string(), serde_json::Value::String(assigned));
            count += 1;
        }
        Ok(count)
    }

    fn override_for(&self, kind: FlagKind, name: &str) -> Option<FlagValue> {
        FlagValue::parse(kind, self.raw(name)?)
    }

    pub fn variable(&mut self, name: &str) -> Option<String> {
        let name = bare(name).to_string();
        let value = self
            .defined
            .iter()
            .find(|((_, n), _)| *n == name)
            .map(|(_, v)| match v {
                FlagValue::Bool(b) => {
                    if *b {
                        "True".to_string()
                    } else {
                        "False".to_string()
                    }
                }
                FlagValue::Int(i) => i.to_string(),
                FlagValue::String(s) => s.clone(),
            })
            .or_else(|| {
                self.raw(&name)
                    .or_else(|| self.engine_defaults.get(&name))
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
            });
        if let Some(v) = &value {
            self.reads.insert(
                format!("FVariable{name}"),
                FlagRead {
                    default: None,
                    value: v.clone(),
                },
            );
        }
        value
    }

    fn record(
        &mut self,
        kind: FlagKind,
        name: &str,
        default: Option<&FlagValue>,
        value: &FlagValue,
    ) {
        let key = format!("{}{}", kind.prefix(), name);
        let entry = self.reads.entry(key).or_insert(FlagRead {
            default: None,
            value: String::new(),
        });
        if let Some(d) = default {
            entry.default = Some(d.describe());
        }
        entry.value = value.describe();
    }

    pub fn define(&mut self, kind: FlagKind, name: &str, default: FlagValue) -> FlagValue {
        let name = bare(name).to_string();
        let value = match self.defined.get(&(kind, name.clone())) {
            Some(v) => v.clone(),
            None => {
                let v = self.override_for(kind, &name).unwrap_or(default.clone());
                self.defined.insert((kind, name.clone()), v.clone());
                v
            }
        };
        self.record(kind, &name, Some(&default), &value);
        value
    }

    pub fn get(&mut self, kind: FlagKind, name: &str) -> FlagValue {
        let name = bare(name).to_string();
        let value = self
            .defined
            .get(&(kind, name.clone()))
            .cloned()
            .or_else(|| self.override_for(kind, &name))
            .unwrap_or_else(|| FlagValue::zero(kind));
        self.record(kind, &name, None, &value);
        value
    }

    pub fn set(&mut self, kind: FlagKind, name: &str, value: FlagValue) -> FlagValue {
        let name = bare(name).to_string();
        let old = self
            .defined
            .insert((kind, name), value)
            .unwrap_or_else(|| FlagValue::zero(kind));
        old
    }

    pub fn set_version(&mut self, name: &str, version: i64) {
        self.versions.insert(bare(name).to_string(), version);
    }

    pub fn engine_feature(&mut self, name: &str) -> bool {
        let on = self
            .engine_features
            .get(name)
            .or_else(|| self.feature_defaults.get(name))
            .copied()
            .unwrap_or(false);
        self.reads.insert(
            format!("EngineFeature:{name}"),
            FlagRead {
                default: None,
                value: on.to_string(),
            },
        );
        on
    }

    pub fn register_layer(&mut self, layer: &str) {
        let key = format!("IXP:{layer}");
        if !self.reads.contains_key(&key) {
            let vars = self.ixp.get(layer).cloned().unwrap_or_default();
            self.reads.insert(
                key,
                FlagRead {
                    default: None,
                    value: serde_json::Value::Object(vars).to_string(),
                },
            );
        }
    }

    pub fn ixp_layer(&mut self, layer: &str) -> serde_json::Map<String, serde_json::Value> {
        let vars = self.ixp.get(layer).cloned().unwrap_or_default();
        self.reads.insert(
            format!("IXP:{layer}"),
            FlagRead {
                default: None,
                value: serde_json::Value::Object(vars.clone()).to_string(),
            },
        );
        vars
    }

    pub fn report(&self) -> String {
        let mut out = String::new();
        for (k, r) in &self.reads {
            out.push_str(&format!(
                "{k}\tdefault={}\tvalue={}\n",
                r.default.as_deref().unwrap_or("-"),
                r.value
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_follow_client_app_settings_format() {
        let mut f = FlagStore::default();
        f.load_json(
            r#"{"FFlagTopBarRefactor": "True", "FIntCount": "7", "EngineFeatures": {"X": true}}"#,
        )
        .expect("json");
        assert_eq!(
            f.define(FlagKind::Flag, "TopBarRefactor", FlagValue::Bool(false)),
            FlagValue::Bool(true)
        );
        assert_eq!(f.get(FlagKind::Int, "FIntCount"), FlagValue::Int(7));
        assert_eq!(
            f.define(FlagKind::Flag, "Other", FlagValue::Bool(true)),
            FlagValue::Bool(true)
        );
        assert!(f.engine_feature("X"));
        assert!(!f.engine_feature("Y"));
    }
}
