// Shared by the host and the relay. The relay may cut very large bodies, leaving JSON
// that no longer parses; top-level string fields are still readable this way.

/// The first `"name": "..."` string value anywhere in `raw`, unescaped.
pub fn string_field(raw: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let mut from = 0;
    while let Some(found) = raw[from..].find(&key) {
        let after = from + found + key.len();
        from = after;
        let rest = raw[after..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let Some(body) = rest.trim_start().strip_prefix('"') else { continue };
        let mut end = None;
        let mut escaped = false;
        for (i, ch) in body.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                end = Some(i);
                break;
            }
        }
        let inner = &body[..end?];
        return Some(serde_json::from_str::<String>(&format!("\"{inner}\"")).unwrap_or_else(|_| inner.to_string()));
    }
    None
}

/// The raw text of the first `"name": [ ... ]` array, without nested arrays.
#[allow(dead_code)] // only the relay binary uses it
pub fn array_field<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\"");
    let at = raw.find(&key)? + key.len();
    let rest = raw[at..].trim_start().strip_prefix(':')?.trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let end = rest.find(']')?;
    Some(&rest[..=end])
}
