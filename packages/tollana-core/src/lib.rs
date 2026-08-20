pub mod container;
pub mod decode;
pub mod exec;
pub mod instruction;
pub mod machine;
pub mod snapshot;
pub mod validate;
pub mod value;

pub use container::{
    decode_container, encode_container, encode_container_aead, ContainerBody, DecodedContainer,
    PluginStateEntry,
};
pub use decode::{decode, decode_binary, decode_text, encode_binary, DecodeError, Module};
pub use exec::{ExecOutcome, HostInterfaceError, Instance, RestoreResult};
pub use instruction::{BlockType, Instruction};
pub use machine::{
    CallFrame, CapabilityTableEntry, Continuation, ControlLabel, ControlLabelKind, HostCall,
    MachineState, ProgramCounter, SuspendReason, TrapKind,
};
pub use snapshot::{
    decode_tirs, encode_tirs, CoreSnapshot, HostRebind, PluginIdentity, SnapshotError,
};
pub use validate::{validate, ValidateError};
pub use value::{CapHandle, Label, Value, ValuePayload, ValueType};
