use crate::error::HostError;
use serde::Deserialize;
use tollana_core::{FunctionType, ValueType};

pub const CLOCK_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/clock.json");
pub const GOAL_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/goal.json");
pub const AI_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/ai.json");
pub const CONTEXT_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/context.json");
pub const FS_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/fs.json");
pub const NET_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/net.json");
pub const RANDOM_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/random.json");
pub const CODE_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/code.json");

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PackageSchema {
    pub name: String,
    pub version: String,
    pub methods: Vec<MethodSchema>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct MethodSchema {
    pub id: u32,
    pub name: String,
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub capabilities: Vec<String>,
}

pub fn parse_package_schema(bytes: &[u8]) -> Result<PackageSchema, HostError> {
    Ok(serde_json::from_slice(bytes)?)
}

pub fn function_type(method: &MethodSchema) -> Result<FunctionType, HostError> {
    Ok(FunctionType {
        parameters: method
            .params
            .iter()
            .map(|s| value_type(s))
            .collect::<Result<Vec<_>, _>>()?,
        results: method
            .results
            .iter()
            .map(|s| value_type(s))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn value_type(name: &str) -> Result<ValueType, HostError> {
    match name {
        "i32" => Ok(ValueType::I32),
        "i64" => Ok(ValueType::I64),
        "unit" => Ok(ValueType::Unit),
        "Capability" => Ok(ValueType::Capability),
        other => Err(HostError::new(format!("unknown schema type {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_schema_fixture_parses() {
        let schema = parse_package_schema(CLOCK_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "clock");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.methods.len(), 2);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "now_wall");
        assert_eq!(schema.methods[0].results, ["i64"]);
        assert_eq!(schema.methods[0].capabilities, ["clock.read"]);
        assert_eq!(schema.methods[1].id, 1);
        assert_eq!(schema.methods[1].name, "now_monotonic");
        assert!(schema.methods.iter().all(|m| m.params.is_empty()));
    }

    #[test]
    fn goal_schema_fixture_parses() {
        let schema = parse_package_schema(GOAL_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "goal");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.methods.len(), 3);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "spawn");
        assert_eq!(schema.methods[0].params, ["i32"]);
        assert_eq!(schema.methods[0].results, ["i32"]);
        assert_eq!(schema.methods[0].capabilities, ["goal.spawn"]);
        assert_eq!(schema.methods[1].name, "join");
        assert_eq!(schema.methods[2].name, "cancel");
        assert_eq!(schema.methods[2].results, ["unit"]);
    }

    #[test]
    fn ai_schema_fixture_parses() {
        let schema = parse_package_schema(AI_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "ai");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.methods.len(), 3);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "chat");
        assert_eq!(schema.methods[0].params, ["i32"]);
        assert_eq!(schema.methods[0].results, ["i32"]);
        assert_eq!(schema.methods[0].capabilities, ["ai.chat"]);
        assert_eq!(schema.methods[1].name, "generate");
        assert_eq!(schema.methods[2].name, "embed");
    }

    #[test]
    fn context_schema_fixture_parses() {
        let schema = parse_package_schema(CONTEXT_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "context");
        assert_eq!(schema.methods.len(), 2);
        assert_eq!(schema.methods[0].name, "list");
        assert!(schema.methods[0].params.is_empty());
        assert_eq!(schema.methods[1].name, "read");
        assert_eq!(schema.methods[1].params, ["i32"]);
        assert_eq!(schema.methods[1].capabilities, ["context.read"]);
    }

    #[test]
    fn fs_schema_fixture_parses() {
        let schema = parse_package_schema(FS_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "fs");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.methods.len(), 3);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "read");
        assert_eq!(schema.methods[0].params, ["Capability", "i32", "i32"]);
        assert_eq!(schema.methods[0].results, ["i32"]);
        assert_eq!(schema.methods[0].capabilities, ["fs.read"]);
        assert_eq!(schema.methods[1].name, "write");
        assert_eq!(
            schema.methods[1].params,
            ["Capability", "i32", "i32", "i32"]
        );
        assert_eq!(schema.methods[1].results, ["unit"]);
        assert_eq!(schema.methods[2].name, "list");
        assert_eq!(schema.methods[2].params, ["Capability"]);
    }

    #[test]
    fn net_schema_fixture_parses() {
        let schema = parse_package_schema(NET_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "net");
        assert_eq!(schema.methods.len(), 1);
        assert_eq!(schema.methods[0].name, "fetch");
        assert_eq!(
            schema.methods[0].params,
            ["Capability", "i32", "i32", "i32", "i32"]
        );
        assert_eq!(schema.methods[0].results, ["i32"]);
        assert_eq!(schema.methods[0].capabilities, ["net.fetch"]);
    }

    #[test]
    fn random_schema_fixture_parses() {
        let schema = parse_package_schema(RANDOM_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "random");
        assert_eq!(schema.methods.len(), 1);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "next");
        assert!(schema.methods[0].params.is_empty());
        assert_eq!(schema.methods[0].results, ["i64"]);
        assert_eq!(schema.methods[0].capabilities, ["random.read"]);
    }

    #[test]
    fn code_schema_fixture_parses() {
        let schema = parse_package_schema(CODE_SCHEMA_BYTES).unwrap();
        assert_eq!(schema.name, "code");
        assert_eq!(schema.version, "1.0.0");
        assert_eq!(schema.methods.len(), 1);
        assert_eq!(schema.methods[0].id, 0);
        assert_eq!(schema.methods[0].name, "run");
        assert_eq!(
            schema.methods[0].params,
            ["i32", "i32", "i32", "i32", "i32"]
        );
        assert_eq!(schema.methods[0].results, ["i32"]);
        assert_eq!(schema.methods[0].capabilities, ["code.run"]);
    }
}
