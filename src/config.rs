use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The subset of Sober's configuration that is relevant to the graphics path.
/// Unknown keys are retained in `raw` so diagnostics can show what was read
/// without pretending to understand every option.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SoberConfig {
    pub use_opengl: Option<bool>,
    pub graphics_optimization_mode: Option<String>,
    pub enable_gamemode: Option<bool>,
    pub enable_hidpi: Option<bool>,
    pub fflags: BTreeMap<String, Value>,
    pub raw: Value,
}

pub fn read(path: impl AsRef<Path>) -> Result<SoberConfig, String> {
    let path = path.as_ref();
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<SoberConfig, String> {
    let cleaned = strip_line_comments(text);
    let raw: Value =
        serde_json::from_str(&cleaned).map_err(|e| format!("invalid Sober config JSON: {e}"))?;
    let obj = raw
        .as_object()
        .ok_or_else(|| "Sober config root is not a JSON object".to_string())?;
    let fflags = obj
        .get("fflags")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    Ok(SoberConfig {
        use_opengl: obj.get("use_opengl").and_then(Value::as_bool),
        graphics_optimization_mode: obj
            .get("graphics_optimization_mode")
            .and_then(Value::as_str)
            .map(str::to_string),
        enable_gamemode: obj.get("enable_gamemode").and_then(Value::as_bool),
        enable_hidpi: obj.get("enable_hidpi").and_then(Value::as_bool),
        fflags,
        raw,
    })
}

/// Sober writes a JSON file preceded by `//` documentation comments. Rust's
/// serde_json does not accept comments, so remove only comments that occur
/// outside string literals. Block comments are not used by the current file.
fn strip_line_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            // Consume the second slash and everything through the newline.
            chars.next();
            while let Some(&next) = chars.peek() {
                if next == '\n' {
                    break;
                }
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_outside_strings() {
        let input = r#"// header
{
  "url": "https://example.test/a//b", // trailing
  "use_opengl": false
}
"#;
        let cfg = parse(input).expect("valid config");
        assert_eq!(cfg.use_opengl, Some(false));
        assert_eq!(cfg.raw["url"], "https://example.test/a//b");
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse("{").is_err());
    }
}
