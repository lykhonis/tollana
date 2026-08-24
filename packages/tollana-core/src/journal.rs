use crate::machine::{ProgramCounter, QuotaDimension, TrapKind};
use crate::snapshot::PluginIdentity;
use crate::value::{Label, Value, ValuePayload, ValueType};

pub trait JournalSink {
    fn append(&mut self, kind: JournalEventKind, sensitivity: Label);
    fn next_sequence(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalValue {
    pub value_type: ValueType,
    pub label: Label,
    pub payload: Option<ValuePayload>,
}

impl JournalValue {
    pub fn from_value(value: &Value, redact: bool) -> Self {
        let hide = redact && value.label >= Label::Confidential;
        Self {
            value_type: value.value_type(),
            label: value.label,
            payload: if hide { None } else { Some(value.payload) },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalEventKind {
    InstructionStepped {
        function_index: u32,
        instruction_index: u32,
        opcode: &'static str,
    },
    FuelSuspended {
        remaining_fuel: u64,
        program_counter: ProgramCounter,
    },
    FuelResumed {
        remaining_fuel: u64,
    },
    QuotaConsumed {
        dimension: QuotaDimension,
        amount: u64,
        remaining: u64,
    },
    QuotaExhausted {
        dimension: QuotaDimension,
        program_counter: ProgramCounter,
    },
    QuotaAdded {
        dimension: QuotaDimension,
        remaining: u64,
    },
    HostCallSuspended {
        plugin_id: u32,
        method_id: u32,
        continuation_identifier: u32,
        arity: u32,
        arguments: Vec<JournalValue>,
    },
    HostCallResumed {
        plugin_id: u32,
        method_id: u32,
        continuation_identifier: u32,
        results: Vec<JournalValue>,
    },
    HostCallFailed {
        plugin_id: u32,
        method_id: u32,
        continuation_identifier: u32,
        message: String,
    },
    Trapped {
        trap_kind: TrapKind,
        program_counter: ProgramCounter,
    },
    Completed {
        results: Vec<JournalValue>,
    },
    SnapshotCoreTaken {
        module_len: u32,
        remaining_fuel: u64,
        memory_len: u32,
        continuation_count: u32,
    },
    SnapshotCoreRestored {
        module_len: u32,
        remaining_fuel: u64,
        memory_len: u32,
        continuation_count: u32,
    },
    SnapshotTaken {
        journal_cursor: u64,
    },
    SnapshotRestored {
        journal_cursor: u64,
    },
    InvalidCapabilityUse {
        table_index: u32,
        generation: u32,
    },
    InstanceCreated {
        plugins: Vec<PluginIdentity>,
    },
}

impl JournalEventKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::InstructionStepped { .. } => "InstructionStepped",
            Self::FuelSuspended { .. } => "FuelSuspended",
            Self::FuelResumed { .. } => "FuelResumed",
            Self::QuotaConsumed { .. } => "QuotaConsumed",
            Self::QuotaExhausted { .. } => "QuotaExhausted",
            Self::QuotaAdded { .. } => "QuotaAdded",
            Self::HostCallSuspended { .. } => "HostCallSuspended",
            Self::HostCallResumed { .. } => "HostCallResumed",
            Self::HostCallFailed { .. } => "HostCallFailed",
            Self::Trapped { .. } => "Trapped",
            Self::Completed { .. } => "Completed",
            Self::SnapshotCoreTaken { .. } => "SnapshotCoreTaken",
            Self::SnapshotCoreRestored { .. } => "SnapshotCoreRestored",
            Self::SnapshotTaken { .. } => "SnapshotTaken",
            Self::SnapshotRestored { .. } => "SnapshotRestored",
            Self::InvalidCapabilityUse { .. } => "InvalidCapabilityUse",
            Self::InstanceCreated { .. } => "InstanceCreated",
        }
    }

    fn redact(self) -> Self {
        match self {
            Self::HostCallSuspended {
                plugin_id,
                method_id,
                continuation_identifier,
                arity,
                arguments,
            } => Self::HostCallSuspended {
                plugin_id,
                method_id,
                continuation_identifier,
                arity,
                arguments: redact_values(arguments),
            },
            Self::HostCallResumed {
                plugin_id,
                method_id,
                continuation_identifier,
                results,
            } => Self::HostCallResumed {
                plugin_id,
                method_id,
                continuation_identifier,
                results: redact_values(results),
            },
            Self::Completed { results } => Self::Completed {
                results: redact_values(results),
            },
            other => other,
        }
    }
}

fn redact_values(values: Vec<JournalValue>) -> Vec<JournalValue> {
    values
        .into_iter()
        .map(|mut v| {
            if v.label >= Label::Confidential {
                v.payload = None;
            }
            v
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEvent {
    pub sequence: u64,
    pub run_id: [u8; 16],
    pub sensitivity: Label,
    pub kind: JournalEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryJournal {
    pub run_id: [u8; 16],
    pub events: Vec<JournalEvent>,
    pub redact: bool,
    pub emit_instruction_stepped: bool,
}

impl Default for MemoryJournal {
    fn default() -> Self {
        Self {
            run_id: [0u8; 16],
            events: Vec::new(),
            redact: true,
            emit_instruction_stepped: false,
        }
    }
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn event_names(&self) -> Vec<&'static str> {
        self.events.iter().map(|e| e.kind.name()).collect()
    }
}

impl JournalSink for MemoryJournal {
    fn append(&mut self, kind: JournalEventKind, sensitivity: Label) {
        let kind = if self.redact { kind.redact() } else { kind };
        let sequence = self.events.len() as u64;
        self.events.push(JournalEvent {
            sequence,
            run_id: self.run_id,
            sensitivity,
            kind,
        });
    }

    fn next_sequence(&self) -> u64 {
        self.events.len() as u64
    }
}

pub fn join_labels(values: &[Value]) -> Label {
    values
        .iter()
        .map(|v| v.label)
        .fold(Label::Public, Label::join)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn host_call_failed_is_named() {
        let mut j = MemoryJournal::new();
        j.append(
            JournalEventKind::HostCallFailed {
                plugin_id: 1,
                method_id: 2,
                continuation_identifier: 0,
                message: "external_confidential".into(),
            },
            Label::Public,
        );
        assert_eq!(j.event_names(), ["HostCallFailed"]);
    }

    #[test]
    fn default_sink_redacts_secret_payload() {
        let mut j = MemoryJournal::new();
        j.append(
            JournalEventKind::HostCallResumed {
                plugin_id: 0,
                method_id: 0,
                continuation_identifier: 0,
                results: vec![JournalValue::from_value(
                    &Value::i32(41, Label::Secret),
                    false,
                )],
            },
            Label::Secret,
        );
        match &j.events[0].kind {
            JournalEventKind::HostCallResumed { results, .. } => {
                assert_eq!(results[0].label, Label::Secret);
                assert_eq!(results[0].payload, None);
            }
            other => panic!("{}", other.name()),
        }
    }

    #[test]
    fn sequence_is_monotonic() {
        let mut j = MemoryJournal::new();
        j.append(
            JournalEventKind::FuelResumed { remaining_fuel: 1 },
            Label::Public,
        );
        j.append(
            JournalEventKind::FuelResumed { remaining_fuel: 2 },
            Label::Public,
        );
        assert_eq!(j.events[0].sequence, 0);
        assert_eq!(j.events[1].sequence, 1);
        assert_eq!(j.next_sequence(), 2);
    }
}
