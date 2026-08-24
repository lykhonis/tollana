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

impl ControlLabelKind {
    pub fn code(self) -> u8 {
        match self {
            ControlLabelKind::Block => 1,
            ControlLabelKind::Loop => 2,
            ControlLabelKind::If => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(ControlLabelKind::Block),
            2 => Some(ControlLabelKind::Loop),
            3 => Some(ControlLabelKind::If),
            _ => None,
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum QuotaDimension {
    MemoryBytes = 1,
    HostCallCount = 2,
    IoBytes = 3,
    Tokens = 4,
    WallTimeMillis = 5,
    ConcurrentGoals = 6,
}

impl QuotaDimension {
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::MemoryBytes),
            2 => Some(Self::HostCallCount),
            3 => Some(Self::IoBytes),
            4 => Some(Self::Tokens),
            5 => Some(Self::WallTimeMillis),
            6 => Some(Self::ConcurrentGoals),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaSlot {
    pub dimension: QuotaDimension,
    pub remaining: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendReason {
    HostInvoke,
    OutOfFuel,
    QuotaExhausted { dimension: QuotaDimension },
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
    pub quotas: Vec<QuotaSlot>,
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
            quotas: Vec::new(),
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
