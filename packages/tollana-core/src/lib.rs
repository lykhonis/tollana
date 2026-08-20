pub mod decode;
pub mod exec;
pub mod instruction;
pub mod machine;
pub mod validate;
pub mod value;

pub use decode::{decode, decode_binary, decode_text, encode_binary, DecodeError, Module};
pub use exec::{ExecOutcome, HostInterfaceError, Instance};
pub use instruction::{BlockType, Instruction};
pub use machine::{
    CallFrame, CapabilityTableEntry, Continuation, ControlLabel, ControlLabelKind, HostCall,
    MachineState, ProgramCounter, SuspendReason, TrapKind,
};
pub use validate::{validate, ValidateError};
pub use value::{CapHandle, Label, Value, ValuePayload, ValueType};
