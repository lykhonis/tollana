use crate::error::HostError;
use serde::Deserialize;
use tollana_core::{FunctionType, ValueType};

pub const CLOCK_SCHEMA_BYTES: &[u8] = include_bytes!("../schemas/clock.json");

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
}
