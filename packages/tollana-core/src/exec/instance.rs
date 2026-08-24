use super::*;
use crate::container::{
    decode_container, encode_container, encode_container_aead, ContainerBody, PluginStateEntry,
};
use crate::decode::{decode_binary, encode_binary, ExportKind, GlobalInit};
use crate::instruction::Instruction;
use crate::journal::{join_labels, JournalEventKind, JournalSink, JournalValue, MemoryJournal};
use crate::machine::{Continuation, QuotaDimension, QuotaSlot, SuspendReason, TrapKind};
use crate::snapshot::{
    capability_handle_live, decode_tirs, encode_tirs, snapshot_capability_values, CoreSnapshot,
    HostRebind,
};
use crate::validate::validate;
use crate::value::{CapHandle, Label, Value};
use std::collections::{HashMap, HashSet};

impl Instance {
    pub fn instantiate(module: Module) -> Result<Self, HostInterfaceError> {
        Self::instantiate_with(module, &[], Vec::new(), DEFAULT_MAX_PAGES, &[])
    }

    pub fn instantiate_with(
        module: Module,
        plugins: &[PluginBinding],
        host_injected_globals: Vec<Value>,
        max_pages: u32,
        quotas: &[QuotaSlot],
    ) -> Result<Self, HostInterfaceError> {
        validate(&module).map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        let mut by_id: HashMap<u32, &PluginBinding> = HashMap::new();
        for binding in plugins {
            if by_id.insert(binding.identity.plugin_id, binding).is_some() {
                return Err(HostInterfaceError::Reject {
                    message: format!("duplicate plugin {}", binding.identity.plugin_id),
                });
            }
        }
        let mut plugin_pairs = HashSet::new();
        for binding in plugins {
            for (method_id, _) in &binding.methods {
                plugin_pairs.insert((binding.identity.plugin_id, *method_id));
            }
        }
        for imp in &module.host_imports {
            if !plugin_pairs.contains(&(imp.plugin_id, imp.method_id)) {
                return Err(HostInterfaceError::Reject {
                    message: format!("missing plugin {} {}", imp.plugin_id, imp.method_id),
                });
            }
            let binding = by_id[&imp.plugin_id];
            let Some((_, method_ty)) = binding
                .methods
                .iter()
                .find(|(method_id, _)| *method_id == imp.method_id)
            else {
                return Err(HostInterfaceError::Reject {
                    message: format!("missing plugin {} {}", imp.plugin_id, imp.method_id),
                });
            };
            let import_ty = &module.types[imp.type_index as usize];
            if method_ty != import_ty {
                return Err(HostInterfaceError::Reject {
                    message: format!(
                        "plugin {} method {} type mismatch",
                        imp.plugin_id, imp.method_id
                    ),
                });
            }
        }
        if let Some(pages) = module.memory_page_count {
            if pages > max_pages {
                return Err(HostInterfaceError::Reject {
                    message: "pageCount exceeds host max".into(),
                });
            }
        }
        let mem_len = module
            .memory_page_count
            .unwrap_or(0)
            .saturating_mul(PAGE_SIZE) as usize;
        let quotas = install_quotas(quotas, mem_len)?;
        let mut injected = host_injected_globals.into_iter();
        let mut globals = Vec::new();
        for g in &module.globals {
            match &g.init {
                GlobalInit::HostInjected => {
                    let v = injected.next().ok_or_else(|| HostInterfaceError::Reject {
                        message: "missing host-injected global".into(),
                    })?;
                    if v.value_type() != g.value_type {
                        return Err(HostInterfaceError::Reject {
                            message: "injected global type mismatch".into(),
                        });
                    }
                    globals.push(v);
                }
                GlobalInit::ConstantExpression(insts) => match insts.as_slice() {
                    [Instruction::I32Const { value }, Instruction::End] => {
                        globals.push(Value::i32(*value, Label::Public));
                    }
                    [Instruction::I64Const { value }, Instruction::End] => {
                        globals.push(Value::i64(*value, Label::Public));
                    }
                    _ => {
                        return Err(HostInterfaceError::Reject {
                            message: "invalid global init".into(),
                        });
                    }
                },
            }
        }
        let mut plugin_identities = Vec::new();
        let mut seen = HashSet::new();
        for imp in &module.host_imports {
            if seen.insert(imp.plugin_id) {
                plugin_identities.push(by_id[&imp.plugin_id].identity.clone());
            }
        }
        let machine = MachineState {
            module_bytes: encode_binary(&module),
            linear_memory: vec![0; mem_len],
            globals,
            remaining_fuel: 0,
            quotas,
            capability_table: Vec::new(),
            pending_host_call: None,
            active_continuation_identifier: None,
            continuations: Vec::new(),
        };
        let mut inst = Self {
            module,
            machine,
            plugins: plugin_pairs,
            plugin_identities,
            entry_export_name: "main".into(),
            last_outcome: None,
            value_stack_cap: VALUE_STACK_CAP,
            call_frame_cap: CALL_FRAME_CAP,
            control_stack_cap: CONTROL_STACK_CAP,
            journal: MemoryJournal::new(),
        };
        let plugins = inst.plugin_identities.clone();
        inst.emit(JournalEventKind::InstanceCreated { plugins }, Label::Public);
        Ok(inst)
    }

    pub fn plugin_identities(&self) -> &[PluginIdentity] {
        &self.plugin_identities
    }

    pub fn invoke(
        &mut self,
        export_name: &str,
        arguments: &[Value],
        initial_fuel: u64,
    ) -> Result<ExecOutcome, HostInterfaceError> {
        if matches!(self.last_outcome, Some(ExecOutcome::Trapped { .. })) {
            return Err(HostInterfaceError::Reject {
                message: "instance trapped".into(),
            });
        }
        let export = self
            .module
            .exports
            .iter()
            .find(|e| e.name == export_name && e.kind == ExportKind::Function)
            .ok_or_else(|| HostInterfaceError::Reject {
                message: format!("missing function export {export_name}"),
            })?;
        let function_index = export.index;
        let func = &self.module.functions[function_index as usize];
        let ty = &self.module.types[func.type_index as usize];
        if arguments.len() != ty.parameters.len() {
            return Err(HostInterfaceError::Reject {
                message: "argument count mismatch".into(),
            });
        }
        for (a, t) in arguments.iter().zip(ty.parameters.iter()) {
            if a.value_type() != *t {
                return Err(HostInterfaceError::Reject {
                    message: "argument type mismatch".into(),
                });
            }
        }
        for a in arguments {
            if let Some(h) = a.as_cap() {
                if !h.is_null() {
                    self.ensure_cap(h, Vec::new());
                }
            }
        }
        self.entry_export_name = export_name.to_string();
        self.machine.remaining_fuel = initial_fuel;
        self.machine.pending_host_call = None;
        self.machine.continuations = vec![Continuation {
            continuation_identifier: 0,
            value_stack: Vec::new(),
            call_frames: Vec::new(),
        }];
        self.machine.active_continuation_identifier = Some(0);
        self.last_outcome = None;
        if let Err(kind) = self.enter_frame(function_index, arguments.to_vec(), None) {
            let pc = ProgramCounter {
                function_index,
                instruction_index: 0,
            };
            return Ok(self.trap(kind, pc));
        }
        self.continue_run()
    }

    pub fn add_fuel(&mut self, amount: u64) {
        self.machine.remaining_fuel = self.machine.remaining_fuel.saturating_add(amount);
        self.emit(
            JournalEventKind::FuelResumed {
                remaining_fuel: self.machine.remaining_fuel,
            },
            Label::Public,
        );
    }

    pub fn add_quota(&mut self, dimension: QuotaDimension, amount: u64) {
        if let Some(slot) = self
            .machine
            .quotas
            .iter_mut()
            .find(|q| q.dimension == dimension)
        {
            slot.remaining = slot.remaining.saturating_add(amount);
        } else {
            self.machine.quotas.push(QuotaSlot {
                dimension,
                remaining: amount,
            });
            self.machine.quotas.sort_by_key(|q| q.dimension);
        }
        let remaining = self.quota_remaining(dimension).unwrap_or(amount);
        self.emit(
            JournalEventKind::QuotaAdded {
                dimension,
                remaining,
            },
            Label::Public,
        );
    }

    pub fn consume_quota(&mut self, dimension: QuotaDimension, amount: u64) -> bool {
        let remaining = {
            let Some(slot) = self
                .machine
                .quotas
                .iter_mut()
                .find(|q| q.dimension == dimension)
            else {
                return true;
            };
            if slot.remaining < amount {
                return false;
            }
            slot.remaining -= amount;
            slot.remaining
        };
        self.emit(
            JournalEventKind::QuotaConsumed {
                dimension,
                amount,
                remaining,
            },
            Label::Public,
        );
        true
    }

    pub fn quota_remaining(&self, dimension: QuotaDimension) -> Option<u64> {
        self.machine
            .quotas
            .iter()
            .find(|q| q.dimension == dimension)
            .map(|q| q.remaining)
    }

    pub fn grant_cap(&mut self, handle: CapHandle, opaque: Vec<u8>) {
        self.ensure_cap(handle, opaque);
    }

    pub fn resume(&mut self, results: Vec<Value>) -> Result<ExecOutcome, HostInterfaceError> {
        let pending = self
            .machine
            .pending_host_call
            .take()
            .ok_or(HostInterfaceError::Reject {
                message: "resume without pending host call".into(),
            })?;
        if !matches!(
            self.last_outcome,
            Some(ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke
            })
        ) {
            self.machine.pending_host_call = Some(pending);
            return Err(HostInterfaceError::Reject {
                message: "resume is only for host.invoke".into(),
            });
        }
        let import = self
            .module
            .host_imports
            .iter()
            .find(|i| i.plugin_id == pending.plugin_id && i.method_id == pending.method_id)
            .ok_or(HostInterfaceError::Reject {
                message: "pending import missing".into(),
            })?;
        let ty = &self.module.types[import.type_index as usize];
        if results.len() != ty.results.len() {
            let pc = self.pc();
            return Ok(self.trap(TrapKind::HostTypeMismatch, pc));
        }
        for (v, t) in results.iter().zip(ty.results.iter()) {
            if v.value_type() != *t {
                let pc = self.pc();
                return Ok(self.trap(TrapKind::HostTypeMismatch, pc));
            }
        }
        for v in &results {
            if let Some(h) = v.as_cap() {
                if !h.is_null() {
                    self.ensure_cap(h, Vec::new());
                }
            }
        }
        let sensitivity = join_labels(&results);
        let journal_results: Vec<JournalValue> = results
            .iter()
            .map(|v| JournalValue::from_value(v, false))
            .collect();
        for v in results {
            if let Err(kind) = self.push(v) {
                let pc = self.pc();
                return Ok(self.trap(kind, pc));
            }
        }
        self.emit(
            JournalEventKind::HostCallResumed {
                results: journal_results,
            },
            sensitivity,
        );
        self.continue_run()
    }

    pub fn snapshot_core(&mut self) -> CoreSnapshot {
        let snap = CoreSnapshot {
            module_bytes: self.machine.module_bytes.clone(),
            entry_name: self.entry_export_name.clone(),
            plugin_identities: self.plugin_identities.clone(),
            remaining_fuel: self.machine.remaining_fuel,
            quotas: self.machine.quotas.clone(),
            linear_memory: self.machine.linear_memory.clone(),
            globals: self.machine.globals.clone(),
            capability_table: self.machine.capability_table.clone(),
            active_continuation_identifier: self.machine.active_continuation_identifier,
            continuations: self.machine.continuations.clone(),
            pending_host_call: self.machine.pending_host_call.clone(),
        };
        self.emit(
            JournalEventKind::SnapshotCoreTaken {
                module_len: snap.module_bytes.len() as u32,
                remaining_fuel: snap.remaining_fuel,
                memory_len: snap.linear_memory.len() as u32,
                continuation_count: snap.continuations.len() as u32,
            },
            Label::Public,
        );
        snap
    }

    pub fn snapshot(&mut self, plugin_state: Vec<PluginStateEntry>) -> Vec<u8> {
        let tirs = encode_tirs(&self.snapshot_core());
        self.emit(
            JournalEventKind::SnapshotTaken {
                journal_cursor: self.journal.next_sequence() + 1,
            },
            Label::Public,
        );
        let journal_cursor = self.journal.next_sequence();
        encode_container(
            &ContainerBody {
                tirs,
                plugin_state,
                journal_cursor,
            },
            [0u8; 16],
        )
    }

    pub fn snapshot_aead(
        &mut self,
        plugin_state: Vec<PluginStateEntry>,
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, HostInterfaceError> {
        let tirs = encode_tirs(&self.snapshot_core());
        self.emit(
            JournalEventKind::SnapshotTaken {
                journal_cursor: self.journal.next_sequence() + 1,
            },
            Label::Public,
        );
        let journal_cursor = self.journal.next_sequence();
        encode_container_aead(
            &ContainerBody {
                tirs,
                plugin_state,
                journal_cursor,
            },
            [0u8; 16],
            key,
            nonce,
        )
        .map_err(|e| HostInterfaceError::Reject { message: e.message })
    }

    pub fn restore(
        bytes: &[u8],
        rebind: &[HostRebind],
        aead_key: Option<&[u8; 32]>,
        journal: Option<MemoryJournal>,
    ) -> Result<RestoreResult, HostInterfaceError> {
        let decoded = decode_container(bytes, aead_key)
            .map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        let core = decode_tirs(&decoded.body.tirs)
            .map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        let mut instance = Self::restore_core(core, rebind, journal)?;
        instance.emit(
            JournalEventKind::SnapshotRestored {
                journal_cursor: decoded.body.journal_cursor,
            },
            Label::Public,
        );
        Ok(RestoreResult {
            instance,
            plugin_state: decoded.body.plugin_state,
            journal_cursor: decoded.body.journal_cursor,
        })
    }

    pub fn restore_core(
        snapshot: CoreSnapshot,
        rebind: &[HostRebind],
        journal: Option<MemoryJournal>,
    ) -> Result<Self, HostInterfaceError> {
        let module = decode_binary(&snapshot.module_bytes)
            .map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        validate(&module).map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        for ident in &snapshot.plugin_identities {
            let rb = rebind.iter().find(|r| r.plugin_id == ident.plugin_id);
            let Some(rb) = rb else {
                return Err(HostInterfaceError::Reject {
                    message: format!("missing rebind for plugin {}", ident.plugin_id),
                });
            };
            if rb.identity_hash != ident.identity_hash {
                return Err(HostInterfaceError::Reject {
                    message: "plugin identity hash mismatch".into(),
                });
            }
        }
        let mut plugins = HashSet::new();
        for rb in rebind {
            for pair in &rb.methods {
                plugins.insert(*pair);
            }
        }
        for imp in &module.host_imports {
            if !plugins.contains(&(imp.plugin_id, imp.method_id)) {
                return Err(HostInterfaceError::Reject {
                    message: format!("missing plugin {} {}", imp.plugin_id, imp.method_id),
                });
            }
        }
        let expected_mem = module
            .memory_page_count
            .unwrap_or(0)
            .saturating_mul(PAGE_SIZE) as usize;
        if snapshot.linear_memory.len() != expected_mem {
            return Err(HostInterfaceError::Reject {
                message: "restored memory length mismatch".into(),
            });
        }
        if snapshot.globals.len() != module.globals.len() {
            return Err(HostInterfaceError::Reject {
                message: "restored global count mismatch".into(),
            });
        }
        for v in snapshot_capability_values(&snapshot) {
            if let Some(h) = v.as_cap() {
                if !capability_handle_live(&snapshot.capability_table, h) {
                    return Err(HostInterfaceError::Reject {
                        message: "capability mismatch".into(),
                    });
                }
            }
        }
        let last_outcome = if snapshot.pending_host_call.is_some() {
            Some(ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            })
        } else if snapshot.active_continuation_identifier.is_some() {
            let reason = if snapshot.remaining_fuel == 0 {
                SuspendReason::OutOfFuel
            } else {
                snapshot
                    .quotas
                    .iter()
                    .find(|q| q.remaining == 0)
                    .map(|q| SuspendReason::QuotaExhausted {
                        dimension: q.dimension,
                    })
                    .unwrap_or(SuspendReason::OutOfFuel)
            };
            Some(ExecOutcome::Suspended { reason })
        } else {
            None
        };
        let module_len = snapshot.module_bytes.len() as u32;
        let remaining_fuel = snapshot.remaining_fuel;
        let memory_len = snapshot.linear_memory.len() as u32;
        let continuation_count = snapshot.continuations.len() as u32;
        let mut inst = Self {
            module,
            machine: MachineState {
                module_bytes: snapshot.module_bytes,
                linear_memory: snapshot.linear_memory,
                globals: snapshot.globals,
                remaining_fuel: snapshot.remaining_fuel,
                quotas: snapshot.quotas,
                capability_table: snapshot.capability_table,
                pending_host_call: snapshot.pending_host_call,
                active_continuation_identifier: snapshot.active_continuation_identifier,
                continuations: snapshot.continuations,
            },
            plugins,
            plugin_identities: snapshot.plugin_identities,
            entry_export_name: snapshot.entry_name,
            last_outcome,
            value_stack_cap: VALUE_STACK_CAP,
            call_frame_cap: CALL_FRAME_CAP,
            control_stack_cap: CONTROL_STACK_CAP,
            journal: journal.unwrap_or_default(),
        };
        inst.emit(
            JournalEventKind::SnapshotCoreRestored {
                module_len,
                remaining_fuel,
                memory_len,
                continuation_count,
            },
            Label::Public,
        );
        Ok(inst)
    }

    pub fn trap_pending(&mut self, kind: TrapKind) -> Result<ExecOutcome, HostInterfaceError> {
        if self.machine.pending_host_call.take().is_none() {
            return Err(HostInterfaceError::Reject {
                message: "trap_pending without pending host call".into(),
            });
        }
        let pc = self.pc();
        Ok(self.trap(kind, pc))
    }
}
