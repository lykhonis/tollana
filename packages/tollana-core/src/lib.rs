pub mod container;
pub mod decode;
pub mod exec;
pub mod identity;
pub mod instruction;
pub mod journal;
pub mod machine;
pub mod snapshot;
pub mod validate;
pub mod value;

pub use container::{
    decode_container, encode_container, encode_container_aead, ContainerBody, DecodedContainer,
    PluginStateEntry,
};
pub use decode::{
    decode, decode_binary, decode_text, encode_binary, DecodeError, FunctionType, Module,
};
pub use exec::{ExecOutcome, HostInterfaceError, Instance, PluginBinding, RestoreResult};
pub use identity::{
    assign_local_ids, encode_plugin_identity, hash_canonical_bytes, hash_plugin_identity,
    IdentityError, PluginIdentityInput, IDENTITY_VERSION,
};
pub use instruction::{BlockType, Instruction};
pub use journal::{
    join_labels, JournalEvent, JournalEventKind, JournalSink, JournalValue, MemoryJournal,
};
pub use machine::{
    CallFrame, CapabilityTableEntry, Continuation, ControlLabel, ControlLabelKind, HostCall,
    MachineState, ProgramCounter, QuotaDimension, QuotaSlot, SuspendReason, TrapKind,
};
pub use snapshot::{
    decode_tirs, encode_tirs, CoreSnapshot, HostRebind, PluginIdentity, SnapshotError,
};
pub use validate::{validate, ValidateError};
pub use value::{CapHandle, Label, Value, ValuePayload, ValueType};
