use crate::error::HostError;
use tollana_core::{CapHandle, FunctionType, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginResult {
    Immediate(Vec<Value>),
    Pending(u64),
}

pub trait Plugin {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn schema(&self) -> &[u8];
    fn metadata(&self) -> &[u8] {
        b""
    }
    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError>;
    fn invoke(
        &mut self,
        method_id: u32,
        args: &[Value],
        caps: &[CapHandle],
    ) -> Result<PluginResult, HostError>;
    fn snapshot_state(&self) -> Vec<u8>;
    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError>;
    fn recorded_samples(&self) -> Vec<(u32, i64)> {
        Vec::new()
    }
}
