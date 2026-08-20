use crate::value::{CapHandle, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramCounter {
    pub function_index: u32,
    pub instruction_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlLabelKind {
    Block,
    Loop,
    If,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlLabel {
    pub label_kind: ControlLabelKind,
    pub parameter_count: u32,
    pub result_count: u32,
    pub stack_height: u32,
    pub branch_instruction_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallFrame {
    pub function_index: u32,
    pub instruction_index: u32,
    pub locals: Vec<Value>,
    pub control_stack: Vec<ControlLabel>,
    pub return_program_counter: Option<ProgramCounter>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Continuation {
    pub continuation_identifier: u32,
    pub value_stack: Vec<Value>,
    pub call_frames: Vec<CallFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityTableEntry {
    pub table_index: u32,
    pub generation: u32,
    pub live: bool,
    pub host_identity_opaque: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCall {
    pub plugin_id: u32,
    pub method_id: u32,
    pub arguments: Vec<Value>,
    pub capabilities: Vec<CapHandle>,
    pub continuation_identifier: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendReason {
    HostInvoke,
    OutOfFuel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapKind {
    UnreachableInstruction,
    OutOfBoundsMemory,
    IntegerDivideByZero,
    IntegerOverflow,
    InvalidCapability,
    ValueStackOverflow,
    ValueStackUnderflow,
    CallStackOverflow,
    ControlStackOverflow,
    TypeMismatch,
    InvalidProgramCounter,
    HostNotFound,
    HostTypeMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineState {
    pub module_bytes: Vec<u8>,
    pub linear_memory: Vec<u8>,
    pub globals: Vec<Value>,
    pub remaining_fuel: u64,
    pub capability_table: Vec<CapabilityTableEntry>,
    pub pending_host_call: Option<HostCall>,
    pub active_continuation_identifier: Option<u32>,
    pub continuations: Vec<Continuation>,
}

impl MachineState {
    pub fn new() -> Self {
        Self {
            module_bytes: Vec::new(),
            linear_memory: Vec::new(),
            globals: Vec::new(),
            remaining_fuel: 0,
            capability_table: Vec::new(),
            pending_host_call: None,
            active_continuation_identifier: None,
            continuations: Vec::new(),
        }
    }
}

impl Default for MachineState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_machine_has_no_pending_host_call() {
        let machine = MachineState::new();
        assert!(machine.pending_host_call.is_none());
        assert!(machine.continuations.is_empty());
        assert_eq!(machine.remaining_fuel, 0);
    }
}
