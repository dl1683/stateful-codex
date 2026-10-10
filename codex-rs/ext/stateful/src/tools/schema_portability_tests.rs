use std::sync::Arc;

use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

use super::RunTools;
use super::project_intelligence_tools;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

/// Keywords every provider bridge we target (OpenAI Responses, Gemini/Vertex function
/// declarations, LiteLLM's Responses-to-Vertex translation) accepts. Unions, references,
/// formats, patterns, and numeric bounds are expressed in descriptions and enforced by
/// argument validation instead.
const PORTABLE_KEYWORDS: [&str; 8] = [
    "type",
    "description",
    "enum",
    "items",
    "minItems",
    "properties",
    "required",
    "additionalProperties",
];
const PORTABLE_TYPES: [&str; 6] = ["string", "integer", "number", "boolean", "object", "array"];
/// Nesting of object/array schemas below the parameters root. Deeper declarations are
/// rejected or silently truncated by some function-declaration translators.
const MAX_PORTABLE_DEPTH: usize = 6;

fn check(schema: &Value, path: &str, depth: usize, violations: &mut Vec<String>) {
    let Some(map) = schema.as_object() else {
        violations.push(format!("{path}: schema is not an object"));
        return;
    };
    if depth > MAX_PORTABLE_DEPTH {
        violations.push(format!("{path}: nested deeper than {MAX_PORTABLE_DEPTH}"));
    }
    for key in map.keys() {
        if !PORTABLE_KEYWORDS.contains(&key.as_str()) {
            violations.push(format!("{path}: non-portable keyword {key}"));
        }
    }
    let schema_type = match map.get("type") {
        Some(Value::String(schema_type)) if PORTABLE_TYPES.contains(&schema_type.as_str()) => {
            schema_type.as_str()
        }
        other => {
            violations.push(format!(
                "{path}: type must be one portable name, got {other:?}"
            ));
            return;
        }
    };
    if let Some(values) = map.get("enum") {
        let portable = schema_type == "string"
            && values
                .as_array()
                .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string));
        if !portable {
            violations.push(format!(
                "{path}: enum must be non-empty strings on a string"
            ));
        }
    }
    if map
        .get("additionalProperties")
        .is_some_and(|value| value != &Value::Bool(false))
    {
        violations.push(format!("{path}: additionalProperties may only be false"));
    }
    match schema_type {
        "object" => {
            let Some(properties) = map
                .get("properties")
                .and_then(Value::as_object)
                .filter(|properties| !properties.is_empty())
            else {
                violations.push(format!("{path}: object needs non-empty properties"));
                return;
            };
            for required in map
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if !required
                    .as_str()
                    .is_some_and(|name| properties.contains_key(name))
                {
                    violations.push(format!("{path}: required {required} is not a property"));
                }
            }
            for (name, property) in properties {
                check(property, &format!("{path}.{name}"), depth + 1, violations);
            }
        }
        "array" => match map.get("items") {
            Some(items) => check(items, &format!("{path}[]"), depth + 1, violations),
            None => violations.push(format!("{path}: array needs items")),
        },
        _ => {
            for key in ["items", "properties", "required", "additionalProperties"] {
                if map.contains_key(key) {
                    violations.push(format!("{path}: {key} on a {schema_type}"));
                }
            }
        }
    }
}

#[test]
fn every_stateful_tool_schema_uses_the_portable_subset() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let tools = project_intelligence_tools(
        "project-1".to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
        RunTools::Offered,
    );
    let mut violations = Vec::new();
    for tool in &tools {
        let spec = serde_json::to_value(tool.spec()).expect("spec serializes");
        let name = tool.tool_name().to_string();
        let parameters = &spec["parameters"];
        if parameters["type"] != "object" {
            violations.push(format!("{name}: parameters root must be an object"));
        }
        check(parameters, &name, /*depth*/ 0, &mut violations);
    }
    assert_eq!(violations, Vec::<String>::new());
}
