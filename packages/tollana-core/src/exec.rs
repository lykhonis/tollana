use crate::decode::{encode_binary, ExportKind, FunctionType, GlobalInit, Module};
use crate::instruction::BlockType;
use crate::instruction::Instruction;
use crate::machine::{
    CallFrame, CapabilityTableEntry, Continuation, ControlLabel, ControlLabelKind, HostCall,
    MachineState, ProgramCounter, SuspendReason, TrapKind,
};
use crate::validate::validate;
use crate::value::{CapHandle, Label, Value};
use std::collections::HashSet;
use std::fmt;

const VALUE_STACK_CAP: usize = 65536;
const CALL_FRAME_CAP: usize = 1024;
const CONTROL_STACK_CAP: usize = 1024;
const DEFAULT_MAX_PAGES: u32 = 16;
const PAGE_SIZE: u32 = 65536;

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

pub struct Instance {
    pub module: Module,
    pub machine: MachineState,
    plugins: HashSet<(u32, u32)>,
    last_outcome: Option<ExecOutcome>,
    value_stack_cap: usize,
    call_frame_cap: usize,
    control_stack_cap: usize,
}

impl Instance {
    pub fn instantiate(module: Module) -> Result<Self, HostInterfaceError> {
        Self::instantiate_with(module, HashSet::new(), Vec::new(), DEFAULT_MAX_PAGES)
    }

    pub fn instantiate_with(
        module: Module,
        plugins: HashSet<(u32, u32)>,
        host_injected_globals: Vec<Value>,
        max_pages: u32,
    ) -> Result<Self, HostInterfaceError> {
        validate(&module).map_err(|e| HostInterfaceError::Reject { message: e.message })?;
        for imp in &module.host_imports {
            if !plugins.contains(&(imp.plugin_id, imp.method_id)) {
                return Err(HostInterfaceError::Reject {
                    message: format!("missing plugin {} {}", imp.plugin_id, imp.method_id),
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
        let machine = MachineState {
            module_bytes: encode_binary(&module),
            linear_memory: vec![0; mem_len],
            globals,
            remaining_fuel: 0,
            capability_table: Vec::new(),
            pending_host_call: None,
            active_continuation_identifier: None,
            continuations: Vec::new(),
        };
        Ok(Self {
            module,
            machine,
            plugins,
            last_outcome: None,
            value_stack_cap: VALUE_STACK_CAP,
            call_frame_cap: CALL_FRAME_CAP,
            control_stack_cap: CONTROL_STACK_CAP,
        })
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
        for v in results {
            if let Err(kind) = self.push(v) {
                let pc = self.pc();
                return Ok(self.trap(kind, pc));
            }
        }
        self.continue_run()
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

    pub fn continue_run(&mut self) -> Result<ExecOutcome, HostInterfaceError> {
        if self.machine.pending_host_call.is_some() {
            return Err(HostInterfaceError::HostCallPending);
        }
        if self.machine.active_continuation_identifier.is_none() {
            return match &self.last_outcome {
                Some(ExecOutcome::Completed { .. }) | Some(ExecOutcome::Trapped { .. }) => {
                    Ok(self.last_outcome.clone().unwrap())
                }
                _ => Err(HostInterfaceError::InstanceIdle),
            };
        }
        loop {
            match self.step() {
                Step::Continue => {}
                Step::Done(outcome) => {
                    self.last_outcome = Some(outcome.clone());
                    return Ok(outcome);
                }
            }
        }
    }

    fn trap(&mut self, kind: TrapKind, pc: ProgramCounter) -> ExecOutcome {
        self.machine.active_continuation_identifier = None;
        ExecOutcome::Trapped {
            trap_kind: kind,
            program_counter: pc,
        }
    }

    fn pc(&self) -> ProgramCounter {
        let frame = self.frame();
        ProgramCounter {
            function_index: frame.function_index,
            instruction_index: frame.instruction_index,
        }
    }

    fn continuation(&self) -> &Continuation {
        &self.machine.continuations[0]
    }

    fn continuation_mut(&mut self) -> &mut Continuation {
        &mut self.machine.continuations[0]
    }

    fn frame(&self) -> &CallFrame {
        self.continuation().call_frames.last().unwrap()
    }

    fn frame_mut(&mut self) -> &mut CallFrame {
        self.continuation_mut().call_frames.last_mut().unwrap()
    }

    fn instructions(&self) -> &[Instruction] {
        let idx = self.frame().function_index as usize;
        &self.module.functions[idx].instructions
    }

    fn current_type(&self) -> &FunctionType {
        let f = &self.module.functions[self.frame().function_index as usize];
        &self.module.types[f.type_index as usize]
    }

    fn push(&mut self, v: Value) -> Result<(), TrapKind> {
        if self.continuation().value_stack.len() >= self.value_stack_cap {
            return Err(TrapKind::ValueStackOverflow);
        }
        self.continuation_mut().value_stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, TrapKind> {
        self.continuation_mut()
            .value_stack
            .pop()
            .ok_or(TrapKind::ValueStackUnderflow)
    }

    fn pop_i32(&mut self) -> Result<Value, TrapKind> {
        let v = self.pop()?;
        if v.as_i32().is_none() {
            return Err(TrapKind::TypeMismatch);
        }
        Ok(v)
    }

    fn pop_i64(&mut self) -> Result<Value, TrapKind> {
        let v = self.pop()?;
        if v.as_i64().is_none() {
            return Err(TrapKind::TypeMismatch);
        }
        Ok(v)
    }

    fn step(&mut self) -> Step {
        let pc = self.pc();
        let inst = {
            let insts = self.instructions();
            if pc.instruction_index as usize >= insts.len() {
                return Step::Done(self.trap(TrapKind::InvalidProgramCounter, pc));
            }
            insts[pc.instruction_index as usize]
        };
        if self.machine.remaining_fuel == 0 {
            return Step::Done(ExecOutcome::Suspended {
                reason: SuspendReason::OutOfFuel,
            });
        }
        self.machine.remaining_fuel -= 1;
        match self.exec(inst) {
            Ok(ExecAction::Next) => {
                self.frame_mut().instruction_index += 1;
                Step::Continue
            }
            Ok(ExecAction::Stay) => Step::Continue,
            Ok(ExecAction::Suspend) => Step::Done(ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            }),
            Ok(ExecAction::Complete(results)) => {
                self.machine.active_continuation_identifier = None;
                self.machine.continuations.clear();
                Step::Done(ExecOutcome::Completed { results })
            }
            Err(kind) => Step::Done(self.trap(kind, pc)),
        }
    }

    fn exec(&mut self, inst: Instruction) -> Result<ExecAction, TrapKind> {
        match inst {
            Instruction::Nop => Ok(ExecAction::Next),
            Instruction::Unreachable => Err(TrapKind::UnreachableInstruction),
            Instruction::Drop => {
                self.pop()?;
                Ok(ExecAction::Next)
            }
            Instruction::I32Const { value } => {
                self.push(Value::i32(value, Label::Public))?;
                Ok(ExecAction::Next)
            }
            Instruction::I64Const { value } => {
                self.push(Value::i64(value, Label::Public))?;
                Ok(ExecAction::Next)
            }
            Instruction::I32Add => self.bin_i32(|a, b| Ok(a.wrapping_add(b))),
            Instruction::I32Sub => self.bin_i32(|a, b| Ok(a.wrapping_sub(b))),
            Instruction::I32Mul => self.bin_i32(|a, b| Ok(a.wrapping_mul(b))),
            Instruction::I32DivS => self.bin_i32(div_i32),
            Instruction::I32RemS => self.bin_i32(rem_i32),
            Instruction::I32Eqz => self.un_i32_test(|a| a == 0),
            Instruction::I32Eq => self.cmp_i32(|a, b| a == b),
            Instruction::I32Ne => self.cmp_i32(|a, b| a != b),
            Instruction::I32LtS => self.cmp_i32(|a, b| a < b),
            Instruction::I32GtS => self.cmp_i32(|a, b| a > b),
            Instruction::I32LeS => self.cmp_i32(|a, b| a <= b),
            Instruction::I32GeS => self.cmp_i32(|a, b| a >= b),
            Instruction::I64Add => self.bin_i64(|a, b| Ok(a.wrapping_add(b))),
            Instruction::I64Sub => self.bin_i64(|a, b| Ok(a.wrapping_sub(b))),
            Instruction::I64Mul => self.bin_i64(|a, b| Ok(a.wrapping_mul(b))),
            Instruction::I64DivS => self.bin_i64(div_i64),
            Instruction::I64RemS => self.bin_i64(rem_i64),
            Instruction::I64Eqz => self.un_i64_test(|a| a == 0),
            Instruction::I64Eq => self.cmp_i64(|a, b| a == b),
            Instruction::I64Ne => self.cmp_i64(|a, b| a != b),
            Instruction::I64LtS => self.cmp_i64(|a, b| a < b),
            Instruction::I64GtS => self.cmp_i64(|a, b| a > b),
            Instruction::I64LeS => self.cmp_i64(|a, b| a <= b),
            Instruction::I64GeS => self.cmp_i64(|a, b| a >= b),
            Instruction::LocalGet { local_index } => {
                let v = self.local(local_index)?;
                self.push(v)?;
                Ok(ExecAction::Next)
            }
            Instruction::LocalSet { local_index } => {
                let v = self.pop()?;
                self.set_local(local_index, v)?;
                Ok(ExecAction::Next)
            }
            Instruction::LocalTee { local_index } => {
                let v = self.pop()?;
                self.set_local(local_index, v)?;
                self.push(v)?;
                Ok(ExecAction::Next)
            }
            Instruction::GlobalGet { global_index } => {
                let v = *self
                    .machine
                    .globals
                    .get(global_index as usize)
                    .ok_or(TrapKind::TypeMismatch)?;
                self.push(v)?;
                Ok(ExecAction::Next)
            }
            Instruction::GlobalSet { global_index } => {
                let v = self.pop()?;
                let slot = self
                    .machine
                    .globals
                    .get_mut(global_index as usize)
                    .ok_or(TrapKind::TypeMismatch)?;
                if v.value_type() != slot.value_type() {
                    return Err(TrapKind::TypeMismatch);
                }
                *slot = v;
                Ok(ExecAction::Next)
            }
            Instruction::End => self.exec_end(),
            Instruction::Return => self.exec_return(),
            Instruction::I32Load { immediate_offset } => {
                let addr = self.pop_i32()?.as_i32().unwrap();
                let at = self.mem_addr(addr, immediate_offset, 4)?;
                let bytes = [
                    self.machine.linear_memory[at],
                    self.machine.linear_memory[at + 1],
                    self.machine.linear_memory[at + 2],
                    self.machine.linear_memory[at + 3],
                ];
                self.push(Value::i32(i32::from_le_bytes(bytes), Label::Public))?;
                Ok(ExecAction::Next)
            }
            Instruction::I32Store { immediate_offset } => {
                let val = self.pop_i32()?.as_i32().unwrap();
                let addr = self.pop_i32()?.as_i32().unwrap();
                let at = self.mem_addr(addr, immediate_offset, 4)?;
                let bytes = val.to_le_bytes();
                self.machine.linear_memory[at..at + 4].copy_from_slice(&bytes);
                Ok(ExecAction::Next)
            }
            Instruction::I64Load { immediate_offset } => {
                let addr = self.pop_i32()?.as_i32().unwrap();
                let at = self.mem_addr(addr, immediate_offset, 8)?;
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&self.machine.linear_memory[at..at + 8]);
                self.push(Value::i64(i64::from_le_bytes(bytes), Label::Public))?;
                Ok(ExecAction::Next)
            }
            Instruction::I64Store { immediate_offset } => {
                let val = self.pop_i64()?.as_i64().unwrap();
                let addr = self.pop_i32()?.as_i32().unwrap();
                let at = self.mem_addr(addr, immediate_offset, 8)?;
                self.machine.linear_memory[at..at + 8].copy_from_slice(&val.to_le_bytes());
                Ok(ExecAction::Next)
            }
            Instruction::MemorySize => {
                let pages = (self.machine.linear_memory.len() / PAGE_SIZE as usize) as i32;
                self.push(Value::i32(pages, Label::Public))?;
                Ok(ExecAction::Next)
            }
            Instruction::Block { block_type } => {
                self.push_structured(ControlLabelKind::Block, block_type, false)?;
                Ok(ExecAction::Next)
            }
            Instruction::Loop { block_type } => {
                self.push_structured(ControlLabelKind::Loop, block_type, true)?;
                Ok(ExecAction::Next)
            }
            Instruction::If { block_type } => {
                let cond = self.pop_i32()?.as_i32().unwrap();
                let (params, results) = self.block_arity(block_type)?;
                let open = self.frame().instruction_index;
                let function_index = self.frame().function_index;
                let end = self.find_end(function_index, open);
                let els = self.find_else(function_index, open);
                let height = self.continuation().value_stack.len() as u32 - params;
                self.push_label(ControlLabel {
                    label_kind: ControlLabelKind::If,
                    parameter_count: params,
                    result_count: results,
                    stack_height: height,
                    branch_instruction_index: end,
                })?;
                if cond != 0 {
                    Ok(ExecAction::Next)
                } else if let Some(e) = els {
                    self.frame_mut().instruction_index = e + 1;
                    Ok(ExecAction::Stay)
                } else {
                    self.frame_mut().instruction_index = end;
                    Ok(ExecAction::Stay)
                }
            }
            Instruction::Else => {
                let label = self
                    .frame_mut()
                    .control_stack
                    .pop()
                    .ok_or(TrapKind::TypeMismatch)?;
                self.frame_mut().instruction_index = label.branch_instruction_index + 1;
                Ok(ExecAction::Stay)
            }
            Instruction::Br { label_depth } => self.exec_br(label_depth),
            Instruction::BrIf { label_depth } => {
                let cond = self.pop_i32()?.as_i32().unwrap();
                if cond != 0 {
                    self.exec_br(label_depth)
                } else {
                    Ok(ExecAction::Next)
                }
            }
            Instruction::Call { function_index } => self.exec_call(function_index),
            Instruction::HostInvoke { host_import_index } => {
                self.exec_host_invoke(host_import_index)
            }
        }
    }

    fn local(&self, index: u32) -> Result<Value, TrapKind> {
        self.frame()
            .locals
            .get(index as usize)
            .copied()
            .ok_or(TrapKind::TypeMismatch)
    }

    fn set_local(&mut self, index: u32, v: Value) -> Result<(), TrapKind> {
        let slot = self
            .frame_mut()
            .locals
            .get_mut(index as usize)
            .ok_or(TrapKind::TypeMismatch)?;
        if v.value_type() != slot.value_type() {
            return Err(TrapKind::TypeMismatch);
        }
        *slot = v;
        Ok(())
    }

    fn ensure_cap(&mut self, handle: CapHandle, opaque: Vec<u8>) {
        if handle.is_null() {
            return;
        }
        if let Some(e) = self
            .machine
            .capability_table
            .iter_mut()
            .find(|e| e.table_index == handle.table_index)
        {
            e.generation = handle.generation;
            e.live = true;
            if !opaque.is_empty() {
                e.host_identity_opaque = opaque;
            }
            return;
        }
        self.machine.capability_table.push(CapabilityTableEntry {
            table_index: handle.table_index,
            generation: handle.generation,
            live: true,
            host_identity_opaque: opaque,
        });
    }

    fn cap_is_live(&self, handle: CapHandle) -> bool {
        !handle.is_null()
            && self.machine.capability_table.iter().any(|e| {
                e.table_index == handle.table_index && e.generation == handle.generation && e.live
            })
    }

    fn mem_addr(&self, addr: i32, offset: u32, size: u64) -> Result<usize, TrapKind> {
        let ea = u64::from(addr as u32).saturating_add(u64::from(offset));
        let len = self.machine.linear_memory.len() as u64;
        if ea.saturating_add(size) > len {
            return Err(TrapKind::OutOfBoundsMemory);
        }
        Ok(ea as usize)
    }

    fn block_arity(&self, bt: BlockType) -> Result<(u32, u32), TrapKind> {
        match bt {
            BlockType::Empty => Ok((0, 0)),
            BlockType::SingleResult(_) => Ok((0, 1)),
            BlockType::TypeIndex(i) => {
                let t = self
                    .module
                    .types
                    .get(i as usize)
                    .ok_or(TrapKind::TypeMismatch)?;
                Ok((t.parameters.len() as u32, t.results.len() as u32))
            }
        }
    }

    fn find_end(&self, function_index: u32, open: u32) -> u32 {
        let insts = &self.module.functions[function_index as usize].instructions;
        let mut depth = 1i32;
        let mut i = open as usize + 1;
        while i < insts.len() {
            match insts[i] {
                Instruction::Block { .. } | Instruction::Loop { .. } | Instruction::If { .. } => {
                    depth += 1;
                }
                Instruction::End => {
                    depth -= 1;
                    if depth == 0 {
                        return i as u32;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        insts.len().saturating_sub(1) as u32
    }

    fn find_else(&self, function_index: u32, open: u32) -> Option<u32> {
        let insts = &self.module.functions[function_index as usize].instructions;
        let mut depth = 1i32;
        let mut i = open as usize + 1;
        while i < insts.len() {
            match insts[i] {
                Instruction::Block { .. } | Instruction::Loop { .. } | Instruction::If { .. } => {
                    depth += 1;
                }
                Instruction::Else if depth == 1 => return Some(i as u32),
                Instruction::End => {
                    depth -= 1;
                    if depth == 0 {
                        return None;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn push_label(&mut self, label: ControlLabel) -> Result<(), TrapKind> {
        if self.frame().control_stack.len() >= self.control_stack_cap {
            return Err(TrapKind::ControlStackOverflow);
        }
        self.frame_mut().control_stack.push(label);
        Ok(())
    }

    fn push_structured(
        &mut self,
        kind: ControlLabelKind,
        block_type: BlockType,
        is_loop: bool,
    ) -> Result<(), TrapKind> {
        let (params, results) = self.block_arity(block_type)?;
        let open = self.frame().instruction_index;
        let function_index = self.frame().function_index;
        let branch = if is_loop {
            open + 1
        } else {
            self.find_end(function_index, open)
        };
        let height = self.continuation().value_stack.len() as u32 - params;
        self.push_label(ControlLabel {
            label_kind: kind,
            parameter_count: params,
            result_count: results,
            stack_height: height,
            branch_instruction_index: branch,
        })
    }

    fn enter_frame(
        &mut self,
        function_index: u32,
        args: Vec<Value>,
        ret: Option<ProgramCounter>,
    ) -> Result<(), TrapKind> {
        if self.continuation().call_frames.len() >= self.call_frame_cap {
            return Err(TrapKind::CallStackOverflow);
        }
        let func = &self.module.functions[function_index as usize];
        let ty = &self.module.types[func.type_index as usize];
        let mut locals = args;
        for t in &func.locals {
            locals.push(Value::default_for(*t));
        }
        let end_index = func.instructions.len().saturating_sub(1) as u32;
        let label = ControlLabel {
            label_kind: ControlLabelKind::Block,
            parameter_count: ty.parameters.len() as u32,
            result_count: ty.results.len() as u32,
            stack_height: self.continuation().value_stack.len() as u32,
            branch_instruction_index: end_index,
        };
        if self.control_stack_cap == 0 {
            return Err(TrapKind::ControlStackOverflow);
        }
        self.continuation_mut().call_frames.push(CallFrame {
            function_index,
            instruction_index: 0,
            locals,
            control_stack: vec![label],
            return_program_counter: ret,
        });
        Ok(())
    }

    fn exec_br(&mut self, label_depth: u32) -> Result<ExecAction, TrapKind> {
        let len = self.frame().control_stack.len();
        let idx = len
            .checked_sub(1 + label_depth as usize)
            .ok_or(TrapKind::TypeMismatch)?;
        if idx == 0 {
            let n = self.current_type().results.len() as u32;
            return self.do_return(n);
        }
        let label = self.frame().control_stack[idx];
        let n = if label.label_kind == ControlLabelKind::Loop {
            label.parameter_count
        } else {
            label.result_count
        };
        let mut kept = Vec::new();
        for _ in 0..n {
            kept.push(self.pop()?);
        }
        kept.reverse();
        self.continuation_mut()
            .value_stack
            .truncate(label.stack_height as usize);
        for v in kept {
            self.push(v)?;
        }
        self.frame_mut()
            .control_stack
            .truncate(if label.label_kind == ControlLabelKind::Loop {
                idx + 1
            } else {
                idx
            });
        if label.label_kind == ControlLabelKind::Loop {
            self.frame_mut().instruction_index = label.branch_instruction_index;
        } else {
            self.frame_mut().instruction_index = label.branch_instruction_index + 1;
        }
        Ok(ExecAction::Stay)
    }

    fn exec_call(&mut self, function_index: u32) -> Result<ExecAction, TrapKind> {
        let expected = {
            let f = self
                .module
                .functions
                .get(function_index as usize)
                .ok_or(TrapKind::TypeMismatch)?;
            let ty = self
                .module
                .types
                .get(f.type_index as usize)
                .ok_or(TrapKind::TypeMismatch)?;
            ty.parameters.clone()
        };
        let n = expected.len();
        let mut args = Vec::new();
        for _ in 0..n {
            args.push(self.pop()?);
        }
        args.reverse();
        for (a, t) in args.iter().zip(expected.iter()) {
            if a.value_type() != *t {
                return Err(TrapKind::TypeMismatch);
            }
        }
        let caller_f = self.frame().function_index;
        let caller_i = self.frame().instruction_index;
        let return_site = ProgramCounter {
            function_index: caller_f,
            instruction_index: caller_i + 1,
        };
        if self.continuation().call_frames.len() >= self.call_frame_cap {
            return Err(TrapKind::CallStackOverflow);
        }
        self.enter_frame(function_index, args, Some(return_site))?;
        let caller_pos = self.continuation().call_frames.len() - 2;
        self.continuation_mut().call_frames[caller_pos].instruction_index =
            return_site.instruction_index;
        Ok(ExecAction::Stay)
    }

    fn exec_host_invoke(&mut self, host_import_index: u32) -> Result<ExecAction, TrapKind> {
        let import = self
            .module
            .host_imports
            .get(host_import_index as usize)
            .ok_or(TrapKind::HostNotFound)?;
        let plugin_id = import.plugin_id;
        let method_id = import.method_id;
        let ty = self
            .module
            .types
            .get(import.type_index as usize)
            .ok_or(TrapKind::HostTypeMismatch)?;
        let params = ty.parameters.clone();
        let mut arguments = Vec::new();
        for _ in 0..params.len() {
            arguments.push(self.pop().map_err(|_| TrapKind::ValueStackUnderflow)?);
        }
        arguments.reverse();
        for (a, t) in arguments.iter().zip(params.iter()) {
            if a.value_type() != *t {
                return Err(TrapKind::HostTypeMismatch);
            }
        }
        let mut capabilities = Vec::new();
        for a in &arguments {
            if let Some(h) = a.as_cap() {
                if !self.cap_is_live(h) {
                    return Err(TrapKind::InvalidCapability);
                }
                capabilities.push(h);
            }
        }
        if !self.plugins.contains(&(plugin_id, method_id)) {
            return Err(TrapKind::HostNotFound);
        }
        self.frame_mut().instruction_index += 1;
        self.machine.pending_host_call = Some(HostCall {
            plugin_id,
            method_id,
            arguments,
            capabilities,
            continuation_identifier: 0,
        });
        Ok(ExecAction::Suspend)
    }

    fn bin_i32(
        &mut self,
        op: fn(i32, i32) -> Result<i32, TrapKind>,
    ) -> Result<ExecAction, TrapKind> {
        let rhs = self.pop_i32()?;
        let lhs = self.pop_i32()?;
        let result = op(lhs.as_i32().unwrap(), rhs.as_i32().unwrap())?;
        let label = lhs.label.join(rhs.label);
        self.push(Value::i32(result, label))?;
        Ok(ExecAction::Next)
    }

    fn bin_i64(
        &mut self,
        op: fn(i64, i64) -> Result<i64, TrapKind>,
    ) -> Result<ExecAction, TrapKind> {
        let rhs = self.pop_i64()?;
        let lhs = self.pop_i64()?;
        let result = op(lhs.as_i64().unwrap(), rhs.as_i64().unwrap())?;
        let label = lhs.label.join(rhs.label);
        self.push(Value::i64(result, label))?;
        Ok(ExecAction::Next)
    }

    fn cmp_i32(&mut self, op: fn(i32, i32) -> bool) -> Result<ExecAction, TrapKind> {
        let rhs = self.pop_i32()?;
        let lhs = self.pop_i32()?;
        let bit = if op(lhs.as_i32().unwrap(), rhs.as_i32().unwrap()) {
            1
        } else {
            0
        };
        let label = lhs.label.join(rhs.label);
        self.push(Value::i32(bit, label))?;
        Ok(ExecAction::Next)
    }

    fn cmp_i64(&mut self, op: fn(i64, i64) -> bool) -> Result<ExecAction, TrapKind> {
        let rhs = self.pop_i64()?;
        let lhs = self.pop_i64()?;
        let bit = if op(lhs.as_i64().unwrap(), rhs.as_i64().unwrap()) {
            1
        } else {
            0
        };
        let label = lhs.label.join(rhs.label);
        self.push(Value::i32(bit, label))?;
        Ok(ExecAction::Next)
    }

    fn un_i32_test(&mut self, op: fn(i32) -> bool) -> Result<ExecAction, TrapKind> {
        let v = self.pop_i32()?;
        let bit = if op(v.as_i32().unwrap()) { 1 } else { 0 };
        self.push(Value::i32(bit, v.label))?;
        Ok(ExecAction::Next)
    }

    fn un_i64_test(&mut self, op: fn(i64) -> bool) -> Result<ExecAction, TrapKind> {
        let v = self.pop_i64()?;
        let bit = if op(v.as_i64().unwrap()) { 1 } else { 0 };
        self.push(Value::i32(bit, v.label))?;
        Ok(ExecAction::Next)
    }

    fn exec_end(&mut self) -> Result<ExecAction, TrapKind> {
        let label = self
            .frame_mut()
            .control_stack
            .pop()
            .ok_or(TrapKind::TypeMismatch)?;
        if self.frame().control_stack.is_empty() {
            return self.do_return(label.result_count);
        }
        Ok(ExecAction::Next)
    }

    fn exec_return(&mut self) -> Result<ExecAction, TrapKind> {
        let n = self.current_type().results.len() as u32;
        self.do_return(n)
    }

    fn do_return(&mut self, result_count: u32) -> Result<ExecAction, TrapKind> {
        let mut results = Vec::new();
        for _ in 0..result_count {
            results.push(self.pop()?);
        }
        results.reverse();
        let expected = self.current_type().results.clone();
        if results.len() != expected.len() {
            return Err(TrapKind::TypeMismatch);
        }
        for (v, t) in results.iter().zip(expected.iter()) {
            if v.value_type() != *t {
                return Err(TrapKind::TypeMismatch);
            }
        }
        self.frame_mut().control_stack.clear();
        let frame = self.continuation_mut().call_frames.pop().unwrap();
        if self.continuation().call_frames.is_empty() {
            return Ok(ExecAction::Complete(results));
        }
        for v in results {
            self.push(v)?;
        }
        if let Some(pc) = frame.return_program_counter {
            self.frame_mut().instruction_index = pc.instruction_index;
        }
        Ok(ExecAction::Stay)
    }
}

enum Step {
    Continue,
    Done(ExecOutcome),
}

enum ExecAction {
    Next,
    Stay,
    Suspend,
    Complete(Vec<Value>),
}

fn div_i32(lhs: i32, rhs: i32) -> Result<i32, TrapKind> {
    if rhs == 0 {
        return Err(TrapKind::IntegerDivideByZero);
    }
    if lhs == i32::MIN && rhs == -1 {
        return Err(TrapKind::IntegerOverflow);
    }
    Ok(lhs.wrapping_div(rhs))
}

fn rem_i32(lhs: i32, rhs: i32) -> Result<i32, TrapKind> {
    if rhs == 0 {
        return Err(TrapKind::IntegerDivideByZero);
    }
    if lhs == i32::MIN && rhs == -1 {
        return Ok(0);
    }
    Ok(lhs.wrapping_rem(rhs))
}

fn div_i64(lhs: i64, rhs: i64) -> Result<i64, TrapKind> {
    if rhs == 0 {
        return Err(TrapKind::IntegerDivideByZero);
    }
    if lhs == i64::MIN && rhs == -1 {
        return Err(TrapKind::IntegerOverflow);
    }
    Ok(lhs.wrapping_div(rhs))
}

fn rem_i64(lhs: i64, rhs: i64) -> Result<i64, TrapKind> {
    if rhs == 0 {
        return Err(TrapKind::IntegerDivideByZero);
    }
    if lhs == i64::MIN && rhs == -1 {
        return Ok(0);
    }
    Ok(lhs.wrapping_rem(rhs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_text;
    use crate::value::{CapHandle, Label};
    use std::collections::HashSet;

    fn run(src: &str, fuel: u64) -> (Instance, ExecOutcome) {
        let module = decode_text(src).expect("decode");
        let mut inst = Instance::instantiate(module).expect("instantiate");
        let out = inst.invoke("main", &[], fuel).expect("invoke");
        (inst, out)
    }

    const ADD: &str = r#"
(module
  (func (export "main") (result i32)
    (i32.add (i32.const 1) (i32.const 2))))
"#;

    #[test]
    fn program_1_add() {
        let (inst, out) = run(ADD, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(3, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 996);
        assert!(inst.machine.pending_host_call.is_none());
    }

    #[test]
    fn program_6_out_of_fuel_then_add_fuel_continue() {
        let module = decode_text(ADD).unwrap();
        let mut inst = Instance::instantiate(module).unwrap();
        let out = inst.invoke("main", &[], 0).unwrap();
        match out {
            ExecOutcome::Suspended {
                reason: SuspendReason::OutOfFuel,
            } => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 0);
        assert_eq!(
            inst.machine.continuations[0].call_frames[0].instruction_index,
            0
        );
        inst.add_fuel(4);
        let out = inst.continue_run().unwrap();
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(3, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 0);
    }

    #[test]
    fn program_6_fuel_one_then_suspend() {
        let (_, out) = run(ADD, 1);
        match out {
            ExecOutcome::Suspended {
                reason: SuspendReason::OutOfFuel,
            } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn continue_after_complete_replays() {
        let (mut inst, first) = run(ADD, 1000);
        let second = inst.continue_run().unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn continue_idle_is_host_error() {
        let module = decode_text(ADD).unwrap();
        let mut inst = Instance::instantiate(module).unwrap();
        assert_eq!(inst.continue_run(), Err(HostInterfaceError::InstanceIdle));
    }

    #[test]
    fn add_joins_labels() {
        let src = r#"
(module
  (func (export "main") (param i32) (param i32) (result i32)
    local.get 0
    local.get 1
    i32.add))
"#;
        let module = decode_text(src).unwrap();
        let mut inst = Instance::instantiate(module).unwrap();
        let out = inst
            .invoke(
                "main",
                &[
                    Value::i32(1, Label::Public),
                    Value::i32(2, Label::Confidential),
                ],
                1000,
            )
            .unwrap();
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results[0], Value::i32(3, Label::Confidential));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn div_by_zero_traps() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (i32.div_s (i32.const 1) (i32.const 0))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Trapped {
                trap_kind: TrapKind::IntegerDivideByZero,
                program_counter,
            } => {
                assert_eq!(program_counter.instruction_index, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn min_div_neg_one_traps() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (i32.div_s (i32.const -2147483648) (i32.const -1))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Trapped {
                trap_kind: TrapKind::IntegerOverflow,
                ..
            } => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn min_rem_neg_one_is_zero() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (i32.rem_s (i32.const -2147483648) (i32.const -1))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(0, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn local_tee_keeps_stack() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (local i32)
    i32.const 9
    local.tee 0
    drop
    local.get 0))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(9, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn program_4a_aligned_store_load() {
        let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.store (i32.const 0) (i32.const 0x01020304))
    (i32.load (i32.const 0))))
"#;
        let (inst, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(0x01020304, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(&inst.machine.linear_memory[0..4], &[0x04, 0x03, 0x02, 0x01]);
        assert_eq!(inst.machine.linear_memory.len(), 65536);
    }

    #[test]
    fn program_4b_unaligned_store_load() {
        let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.store (i32.const 1) (i32.const 0x01020304))
    (i32.load (i32.const 1))))
"#;
        let (inst, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(0x01020304, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.linear_memory[0], 0);
        assert_eq!(&inst.machine.linear_memory[1..5], &[0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn program_5_oob_load_traps() {
        let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.load (i32.const 65535))))
"#;
        let (inst, out) = run(src, 1000);
        match out {
            ExecOutcome::Trapped {
                trap_kind: TrapKind::OutOfBoundsMemory,
                program_counter,
            } => {
                assert_eq!(program_counter.instruction_index, 1);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 998);
        assert!(inst.machine.pending_host_call.is_none());
    }

    #[test]
    fn if_else_and_br_to_function() {
        let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 1
    (if (result i32) (i32.const 10) else (i32.const 20))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(10, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 0
    (if (result i32) (i32.const 10) else (i32.const 20))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(20, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 7
    br 0))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(7, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let src = r#"
(module
  (func (export "main") (result i32)
    (block
      i32.const 9
      return)
    i32.const 1))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(9, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn memory_size_is_page_count() {
        let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    memory.size))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(1, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn i64_store_load() {
        let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i64)
    (i64.store (i32.const 8) (i64.const 0x0102030405060708))
    (i64.load (i32.const 8))))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(0x0102030405060708, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn loop_counts_to_three() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (local i32)
    i32.const 0
    local.set 0
    (block
      (loop
        local.get 0
        i32.const 3
        i32.ge_s
        br_if 1
        local.get 0
        i32.const 1
        i32.add
        local.set 0
        br 0))
    local.get 0))
"#;
        let (_, out) = run(src, 10000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(3, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn call_adds() {
        let src = r#"
(module
  (func (export "main") (result i32)
    (call 1 (i32.const 2) (i32.const 3)))
  (func (param i32) (param i32) (result i32)
    local.get 0
    local.get 1
    i32.add))
"#;
        let (_, out) = run(src, 1000);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(5, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    fn with_echo(src: &str) -> Instance {
        let module = decode_text(src).unwrap();
        let mut plugins = HashSet::new();
        plugins.insert((0, 0));
        Instance::instantiate_with(module, plugins, Vec::new(), 16).unwrap()
    }

    #[test]
    fn program_2_echo() {
        let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;
        let mut inst = with_echo(src);
        let out = inst.invoke("main", &[], 1000).unwrap();
        match out {
            ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            } => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 998);
        let call = inst.machine.pending_host_call.as_ref().unwrap();
        assert_eq!(call.arguments, vec![Value::i32(41, Label::Public)]);
        assert_eq!(
            inst.machine.continuations[0].call_frames[0].instruction_index,
            2
        );
        let out = inst.resume(vec![Value::i32(41, Label::Public)]).unwrap();
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(42, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(inst.machine.remaining_fuel, 995);
        inst = with_echo(src);
        let _ = inst.invoke("main", &[], 1000).unwrap();
        assert_eq!(
            inst.continue_run(),
            Err(HostInterfaceError::HostCallPending)
        );
        let out = inst.trap_pending(TrapKind::HostTypeMismatch).unwrap();
        match out {
            ExecOutcome::Trapped {
                trap_kind: TrapKind::HostTypeMismatch,
                ..
            } => {}
            other => panic!("{other:?}"),
        }
        assert!(inst.machine.pending_host_call.is_none());
    }

    #[test]
    fn program_7_capability_round_trip() {
        let src = r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke UseCapability (local.get 0))))
"#;
        let mut inst = with_echo(src);
        let handle = CapHandle {
            table_index: 1,
            generation: 1,
        };
        inst.grant_cap(handle, b"echo-cap".to_vec());
        let cap = Value::capability(handle, Label::Confidential);
        let out = inst.invoke("main", &[cap], 1000).unwrap();
        match out {
            ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            } => {}
            other => panic!("{other:?}"),
        }
        let call = inst.machine.pending_host_call.as_ref().unwrap();
        assert_eq!(call.arguments[0], cap);
        assert_eq!(call.capabilities, vec![handle]);
        let out = inst.resume(vec![Value::i32(7, Label::Public)]).unwrap();
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(7, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn null_capability_traps_on_invoke() {
        let src = r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (result i32)
    (local Capability)
    (host.invoke UseCapability (local.get 0))))
"#;
        let mut inst = with_echo(src);
        let out = inst.invoke("main", &[], 1000).unwrap();
        match out {
            ExecOutcome::Trapped {
                trap_kind: TrapKind::InvalidCapability,
                program_counter,
            } => {
                assert_eq!(program_counter.instruction_index, 1);
            }
            other => panic!("{other:?}"),
        }
        assert!(inst.machine.pending_host_call.is_none());
    }
}
