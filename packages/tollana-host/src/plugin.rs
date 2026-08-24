use crate::error::HostError;
use tollana_core::{CapHandle, ExecOutcome, FunctionType, QuotaDimension, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginResult {
    Immediate(Vec<Value>),
    Pending(u64),
}

pub trait PluginContext {
    fn caller_continuation(&self) -> u32;
    fn spawn_export(
        &mut self,
        export: &str,
        args: &[Value],
    ) -> Result<(u32, ExecOutcome), HostError>;
    fn cancel_continuation(&mut self, id: u32) -> Result<(), HostError>;
    fn function_export_name(&self, function_index: u32) -> Result<String, HostError>;
    fn consume_quota(&mut self, dimension: QuotaDimension, amount: u64) -> bool;
    fn add_quota(&mut self, dimension: QuotaDimension, amount: u64);
    fn quota_remaining(&self, dimension: QuotaDimension) -> Option<u64>;
    fn live_capabilities(&self) -> Vec<CapHandle>;
    fn read_memory(&self, ptr: i32, len: i32) -> Result<Vec<u8>, HostError> {
        let _ = (ptr, len);
        Err(HostError::new("plugin context has no linear memory"))
    }
    fn write_memory(&mut self, ptr: i32, bytes: &[u8]) -> Result<(), HostError> {
        let _ = (ptr, bytes);
        Err(HostError::new("plugin context has no linear memory"))
    }
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
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError>;
    fn snapshot_state(&self) -> Vec<u8>;
    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError>;
    fn recorded_samples(&self) -> Vec<(u32, i64)> {
        Vec::new()
    }
    fn on_continuation_completed(&mut self, _continuation_id: u32, _results: &[Value]) {}
    fn capability_allowlist(&self, _continuation_id: u32) -> Option<Vec<CapHandle>> {
        None
    }
    fn charge_host_call(&mut self, _continuation_id: u32) -> Result<(), HostError> {
        Ok(())
    }
    fn take_quota_credits(&mut self) -> Vec<(QuotaDimension, u64)> {
        Vec::new()
    }
}
