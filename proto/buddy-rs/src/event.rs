use serde_json::{Map, Value};

pub struct HookEvent {
    map: Option<Map<String, Value>>,
    raw: String,
}

impl HookEvent {
    pub fn parse(raw: &str) -> HookEvent {
        let map = match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(m)) => Some(m),
            _ => None,
        };
        HookEvent { map, raw: raw.to_string() }
    }

    pub fn from_map(map: Map<String, Value>) -> HookEvent {
        HookEvent { map: Some(map), raw: String::new() }
    }

    /// Builds an event from name/value pairs; `workspace_roots` becomes a one-item list.
    #[cfg(test)]
    pub fn from_pairs(pairs: &[&str]) -> HookEvent {
        let mut map = Map::new();
        for pair in pairs.chunks_exact(2) {
            let value = if pair[0] == "workspace_roots" {
                Value::Array(vec![Value::String(pair[1].to_string())])
            } else {
                Value::String(pair[1].to_string())
            };
            map.insert(pair[0].to_string(), value);
        }
        HookEvent::from_map(map)
    }

    pub fn get(&self, name: &str) -> String {
        if let Some(map) = &self.map {
            return match map.get(name) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::Bool(b)) => b.to_string(),
                _ => String::new(),
            };
        }
        crate::jsonscan::string_field(&self.raw, name).unwrap_or_default()
    }

    pub fn value(&self, name: &str) -> Option<&Value> {
        self.map.as_ref().and_then(|m| m.get(name))
    }

    pub fn workspace_roots(&self) -> Vec<String> {
        match self.value("workspace_roots") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}
