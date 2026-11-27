//! The tool registry (P3-T01): unique, well-formed names; strict schemas; argument validation.

use std::collections::BTreeMap;
use std::sync::Arc;

use loki_core::{ToolRequest, ToolSpec};
use serde_json::Value;

use crate::tool::Tool;

/// Longest string a schema may allow without an `enum`. Keeps arguments small and reviewable.
pub const MAX_SCHEMA_STRING: u64 = 4096;
/// Most items a schema array may allow.
pub const MAX_SCHEMA_ITEMS: u64 = 256;

/// Why a tool could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// A tool with this name exists.
    #[error("duplicate tool name '{0}'")]
    Duplicate(String),
    /// The name doesn't match `^[a-z]+(\.[a-z_]+)+$`.
    #[error("invalid tool name '{0}': must match ^[a-z]+(\\.[a-z_]+)+$")]
    BadName(String),
    /// The schema isn't strict enough (see [`check_strict_schema`]).
    #[error("schema of '{tool}' is not strict at {at}: {problem}")]
    LooseSchema {
        /// The tool.
        tool: String,
        /// JSON pointer to the offending part.
        at: String,
        /// What is wrong.
        problem: String,
    },
    /// The schema doesn't compile.
    #[error("schema of '{tool}' is invalid: {detail}")]
    InvalidSchema {
        /// The tool.
        tool: String,
        /// The compiler's message.
        detail: String,
    },
}

/// Why a request was refused before policy ran (SEC-04, SEC-06).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// No tool has this name.
    #[error("unknown tool '{0}'")]
    UnknownTool(String),
    /// The arguments break the tool's schema (wrong type, unknown field, too long...).
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
}

/// `^[a-z]+(\.[a-z_]+)+$`, without a regex dependency.
pub fn is_valid_tool_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let first_ok = parts
        .next()
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()));
    let rest: Vec<&str> = parts.collect();
    first_ok
        && !rest.is_empty()
        && rest
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
}

/// Checks that a schema is closed and bounded everywhere:
/// - the root is an object schema;
/// - every object schema has `"additionalProperties": false` and a `properties` map;
/// - every string has `maxLength <= 4096` or an `enum`;
/// - every array has `maxItems <= 256` and an `items` schema;
/// - every integer/number has `minimum` and `maximum`;
/// - no `$ref` (nothing is fetched or resolved) and no type unions.
pub fn check_strict_schema(schema: &Value) -> Result<(), (String, String)> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err((
            String::from(""),
            String::from("root must be {\"type\": \"object\"}"),
        ));
    }
    check_node(schema, "")
}

fn check_node(node: &Value, at: &str) -> Result<(), (String, String)> {
    let fail = |problem: &str| Err((at.to_owned(), problem.to_owned()));
    let Some(object) = node.as_object() else {
        return fail("schema must be an object");
    };
    if object.contains_key("$ref") || object.contains_key("$dynamicRef") {
        return fail("$ref is not allowed");
    }
    let bounded = |key: &str, max: u64| {
        object
            .get(key)
            .and_then(Value::as_u64)
            .is_some_and(|n| n <= max)
    };
    match object.get("type").and_then(Value::as_str) {
        Some("object") => {
            if object.get("additionalProperties") != Some(&Value::Bool(false)) {
                return fail("objects need \"additionalProperties\": false");
            }
            let Some(properties) = object.get("properties").and_then(Value::as_object) else {
                return fail("objects need a \"properties\" map (it may be empty)");
            };
            for (name, child) in properties {
                check_node(child, &format!("{at}/properties/{name}"))?;
            }
            Ok(())
        }
        Some("string") => {
            if object.contains_key("enum") || bounded("maxLength", MAX_SCHEMA_STRING) {
                Ok(())
            } else {
                fail("strings need maxLength <= 4096 (or an enum)")
            }
        }
        Some("array") => {
            if !bounded("maxItems", MAX_SCHEMA_ITEMS) {
                return fail("arrays need maxItems <= 256");
            }
            match object.get("items") {
                Some(items) => check_node(items, &format!("{at}/items")),
                None => fail("arrays need an items schema"),
            }
        }
        Some("integer" | "number") => {
            if object.contains_key("minimum") && object.contains_key("maximum") {
                Ok(())
            } else {
                fail("numbers need minimum and maximum")
            }
        }
        Some("boolean") => Ok(()),
        Some(other) => fail(&format!("type '{other}' is not allowed")),
        None => fail("every schema needs a single \"type\""),
    }
}

struct Entry {
    tool: Arc<dyn Tool>,
    validator: jsonschema::Validator,
}

/// The set of tools the model may call.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<String, Entry>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.tools.keys()).finish()
    }
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tool after checking its name and schema.
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let spec = tool.spec();
        let name = spec.name.as_str().to_owned();
        if !is_valid_tool_name(&name) {
            return Err(RegistryError::BadName(name));
        }
        if self.tools.contains_key(&name) {
            return Err(RegistryError::Duplicate(name));
        }
        check_strict_schema(&spec.input_schema).map_err(|(at, problem)| {
            RegistryError::LooseSchema {
                tool: name.clone(),
                at: if at.is_empty() { String::from("/") } else { at },
                problem,
            }
        })?;
        let validator = jsonschema::validator_for(&spec.input_schema).map_err(|error| {
            RegistryError::InvalidSchema {
                tool: name.clone(),
                detail: error.to_string(),
            }
        })?;
        self.tools.insert(name, Entry { tool, validator });
        Ok(())
    }

    /// A registered tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name).map(|entry| &entry.tool)
    }

    /// All specs, sorted by name (what the model is offered).
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|entry| entry.tool.spec().clone())
            .collect()
    }

    /// Validates a request's tool name and arguments. Runs **before** policy.
    pub fn validate(&self, request: &ToolRequest) -> Result<&Arc<dyn Tool>, ValidationError> {
        let entry = self
            .tools
            .get(request.tool.as_str())
            .ok_or_else(|| ValidationError::UnknownTool(request.tool.to_string()))?;
        let errors: Vec<String> = entry
            .validator
            .iter_errors(&request.arguments)
            .take(5)
            .map(|error| {
                let path = error.instance_path().to_string();
                if path.is_empty() {
                    error.to_string()
                } else {
                    format!("{path}: {error}")
                }
            })
            .collect();
        if errors.is_empty() {
            Ok(&entry.tool)
        } else {
            Err(ValidationError::InvalidArguments(errors.join("; ")))
        }
    }
}
