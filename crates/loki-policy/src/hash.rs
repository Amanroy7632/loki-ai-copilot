//! Canonical JSON and the approval `args_hash` (CONTRACTS.md "Approval binding").
//!
//! `args_hash = sha256(canonical_json({"arguments": ..., "tool": ...}))`, lowercase hex.
//! Canonical form: object keys sorted by byte order, no whitespace, serde_json's number and
//! string encoding. It is implemented explicitly instead of relying on `serde_json::Map` order,
//! because a dependency enabling `preserve_order` anywhere in the build would silently change it.

use loki_core::ToolName;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Writes `value` in canonical form.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        // Scalars have exactly one serde_json encoding, without whitespace.
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// The hash an approval is bound to. Any change to the tool or any argument changes it.
pub fn args_hash(tool: &ToolName, arguments: &Value) -> String {
    let mut bound = serde_json::Map::new();
    bound.insert(String::from("arguments"), arguments.clone());
    bound.insert(
        String::from("tool"),
        Value::String(tool.as_str().to_owned()),
    );
    let digest = Sha256::digest(canonical_json(&Value::Object(bound)).as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_form_sorts_keys_and_drops_whitespace() {
        let value: Value =
            serde_json::from_str(r#"{ "b": 1, "a": { "y": [1, 2], "x": "s" } }"#).unwrap();
        assert_eq!(canonical_json(&value), r#"{"a":{"x":"s","y":[1,2]},"b":1}"#);
    }

    #[test]
    fn key_order_in_the_input_does_not_matter() {
        let a: Value = serde_json::from_str(r#"{"root_id":"docs","query":"q","limit":5}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"limit":5,"query":"q","root_id":"docs"}"#).unwrap();
        let tool = ToolName::new("files.search");
        assert_eq!(args_hash(&tool, &a), args_hash(&tool, &b));
    }

    #[test]
    fn any_change_changes_the_hash() {
        let tool = ToolName::new("app.open");
        let base = args_hash(&tool, &json!({ "app_id": "com.apple.calculator" }));
        assert_eq!(base.len(), 64);
        assert_ne!(
            base,
            args_hash(&tool, &json!({ "app_id": "com.apple.Terminal" }))
        );
        assert_ne!(
            base,
            args_hash(
                &ToolName::new("app.close"),
                &json!({ "app_id": "com.apple.calculator" })
            )
        );
        assert_ne!(
            base,
            args_hash(
                &tool,
                &json!({ "app_id": "com.apple.calculator", "x": null })
            )
        );
        // Type matters: 5 and "5" are different arguments.
        assert_ne!(
            args_hash(&tool, &json!({ "n": 5 })),
            args_hash(&tool, &json!({ "n": "5" }))
        );
    }

    /// Vectors computed independently: `printf '%s' '<canonical>' | shasum -a 256`.
    #[test]
    fn known_vectors() {
        assert_eq!(
            args_hash(&ToolName::new("time.now"), &json!({})),
            "8715dacde086511c3582bb3ded827ab6b8f771d9cb2a4c88ae5e173836b24534"
        );
        assert_eq!(
            args_hash(
                &ToolName::new("app.open"),
                &json!({ "app_id": "com.apple.calculator" })
            ),
            "18e3d78714f86c5c2ae40900bb974662e664e98fd260c901475aaab0015be696"
        );
    }
}
