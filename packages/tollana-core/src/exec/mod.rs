mod instance;
mod interpret;
mod ops;
#[cfg(test)]
mod tests;

use crate::decode::{FunctionType, Module};
use crate::journal::MemoryJournal;
use crate::machine::{
    MachineState, ProgramCounter, QuotaDimension, QuotaSlot, SuspendReason, TrapKind,
};
use crate::snapshot::PluginIdentity;
use crate::value::Value;
use std::collections::HashSet;
use std::fmt;

pub(crate) const VALUE_STACK_CAP: usize = 65536;
pub(crate) const CALL_FRAME_CAP: usize = 1024;
pub(crate) const CONTROL_STACK_CAP: usize = 1024;
pub(crate) const DEFAULT_MAX_PAGES: u32 = 16;
pub(crate) const PAGE_SIZE: u32 = 65536;

pub(crate) fn install_quotas(
    quotas: &[QuotaSlot],
    mem_len: usize,
) -> Result<Vec<QuotaSlot>, HostInterfaceError> {
    let mut out = quotas.to_vec();
    out.sort_by_key(|q| q.dimension);
    for w in out.windows(2) {
        if w[0].dimension == w[1].dimension {
            return Err(HostInterfaceError::Reject {
                message: "duplicate quota dimension".into(),
            });
        }
    }
    for slot in &mut out {
        if slot.dimension == QuotaDimension::MemoryBytes {
            let used = mem_len as u64;
            if slot.remaining < used {
                return Err(HostInterfaceError::Reject {
                    message: "memory quota below allocated linear memory".into(),
                });
            }
            slot.remaining -= used;
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    Completed {
        results: Vec<Value>,
    },
    Suspended {
        reason: SuspendReason,
    },
    Trapped {
        trap_kind: TrapKind,
        program_counter: ProgramCounter,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostInterfaceError {
    HostCallPending,
    InstanceIdle,
    Reject { message: String },
}

impl fmt::Display for HostInterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostInterfaceError::HostCallPending => write!(f, "HostCallPending"),
            HostInterfaceError::InstanceIdle => write!(f, "InstanceIdle"),
            HostInterfaceError::Reject { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostInterfaceError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginBinding {
    pub identity: PluginIdentity,
    pub methods: Vec<(u32, FunctionType)>,
}

pub struct RestoreResult {
    pub instance: Instance,
    pub plugin_state: Vec<crate::container::PluginStateEntry>,
    pub journal_cursor: u64,
}

pub struct Instance {
    pub module: Module,
    pub machine: MachineState,
    plugins: HashSet<(u32, u32)>,
    plugin_identities: Vec<PluginIdentity>,
    entry_export_name: String,
    last_outcome: Option<ExecOutcome>,
    value_stack_cap: usize,
    call_frame_cap: usize,
    control_stack_cap: usize,
    pub journal: MemoryJournal,
}

pub(crate) enum Step {
    Continue,
    Done(ExecOutcome),
}

pub(crate) enum ExecAction {
    Next,
    Stay,
    Suspend,
    Complete(Vec<Value>),
}
