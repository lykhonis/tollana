use crate::error::HostError;
use crate::plugin::{Plugin, PluginResult};
use crate::schema::{function_type, parse_package_schema, CLOCK_SCHEMA_BYTES};
use std::time::{SystemTime, UNIX_EPOCH};
use tollana_core::{CapHandle, FunctionType, Label, Value};

pub const METHOD_NOW_WALL: u32 = 0;
pub const METHOD_NOW_MONOTONIC: u32 = 1;

#[derive(Clone, Debug)]
pub struct Clock {
    is_virtual: bool,
    wall_millis: i64,
    monotonic_millis: i64,
    samples: Vec<(u32, i64)>,
}

impl Clock {
    pub fn virtual_at(wall_millis: i64, monotonic_millis: i64) -> Self {
        Self {
            is_virtual: true,
            wall_millis,
            monotonic_millis,
            samples: Vec::new(),
        }
    }

    pub fn wall() -> Self {
        Self {
            is_virtual: false,
            wall_millis: 0,
            monotonic_millis: 0,
            samples: Vec::new(),
        }
    }

    pub fn advance(&mut self, millis: i64) {
        self.wall_millis = self.wall_millis.saturating_add(millis);
        self.monotonic_millis = self.monotonic_millis.saturating_add(millis);
    }

    fn read_wall(&self) -> i64 {
        if self.is_virtual {
            self.wall_millis
        } else {
            system_wall_millis()
        }
    }

    fn read_monotonic(&mut self) -> i64 {
        if self.is_virtual {
            self.monotonic_millis
        } else {
            let now = system_wall_millis();
            if self.wall_millis == 0 {
                self.wall_millis = now;
            }
            now.saturating_sub(self.wall_millis)
        }
    }
}

fn system_wall_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Plugin for Clock {
    fn name(&self) -> &str {
        "clock"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        CLOCK_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(CLOCK_SCHEMA_BYTES)?;
        schema
            .methods
            .iter()
            .map(|m| Ok((m.id, function_type(m)?)))
            .collect()
    }

    fn invoke(
        &mut self,
        method_id: u32,
        args: &[Value],
        _caps: &[CapHandle],
    ) -> Result<PluginResult, HostError> {
        if !args.is_empty() {
            return Err(HostError::new("clock methods take no arguments"));
        }
        let value = match method_id {
            METHOD_NOW_WALL => self.read_wall(),
            METHOD_NOW_MONOTONIC => self.read_monotonic(),
            other => return Err(HostError::new(format!("unknown clock method {other}"))),
        };
        self.samples.push((method_id, value));
        Ok(PluginResult::Immediate(vec![Value::i64(
            value,
            Label::Public,
        )]))
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(17);
        buf.push(u8::from(self.is_virtual));
        buf.extend_from_slice(&self.wall_millis.to_le_bytes());
        buf.extend_from_slice(&self.monotonic_millis.to_le_bytes());
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.len() != 17 {
            return Err(HostError::new("invalid clock snapshot blob"));
        }
        self.is_virtual = bytes[0] != 0;
        let mut wall = [0u8; 8];
        wall.copy_from_slice(&bytes[1..9]);
        let mut monotonic = [0u8; 8];
        monotonic.copy_from_slice(&bytes[9..17]);
        self.wall_millis = i64::from_le_bytes(wall);
        self.monotonic_millis = i64::from_le_bytes(monotonic);
        Ok(())
    }

    fn recorded_samples(&self) -> Vec<(u32, i64)> {
        self.samples.clone()
    }
}
