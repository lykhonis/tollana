use super::*;
use crate::instruction::{BlockType, Instruction};
use crate::journal::{join_labels, JournalEventKind, JournalValue};
use crate::machine::{
    CallFrame, CapabilityTableEntry, ControlLabel, ControlLabelKind, HostCall, ProgramCounter,
    QuotaDimension, TrapKind,
};
use crate::value::{CapHandle, Label, Value};

impl Instance {
    pub(crate) fn exec(&mut self, inst: Instruction) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn local(&self, index: u32) -> Result<Value, TrapKind> {
        self.frame()
            .locals
            .get(index as usize)
            .copied()
            .ok_or(TrapKind::TypeMismatch)
    }

    pub(crate) fn set_local(&mut self, index: u32, v: Value) -> Result<(), TrapKind> {
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

    pub(crate) fn ensure_cap(&mut self, handle: CapHandle, opaque: Vec<u8>) {
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

    pub(crate) fn cap_is_live(&self, handle: CapHandle) -> bool {
        !handle.is_null()
            && self.machine.capability_table.iter().any(|e| {
                e.table_index == handle.table_index && e.generation == handle.generation && e.live
            })
    }

    pub(crate) fn mem_addr(&self, addr: i32, offset: u32, size: u64) -> Result<usize, TrapKind> {
        let ea = u64::from(addr as u32).saturating_add(u64::from(offset));
        let len = self.machine.linear_memory.len() as u64;
        if ea.saturating_add(size) > len {
            return Err(TrapKind::OutOfBoundsMemory);
        }
        Ok(ea as usize)
    }

    pub(crate) fn block_arity(&self, bt: BlockType) -> Result<(u32, u32), TrapKind> {
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

    pub(crate) fn find_end(&self, function_index: u32, open: u32) -> u32 {
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

    pub(crate) fn find_else(&self, function_index: u32, open: u32) -> Option<u32> {
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

    pub(crate) fn push_label(&mut self, label: ControlLabel) -> Result<(), TrapKind> {
        if self.frame().control_stack.len() >= self.control_stack_cap {
            return Err(TrapKind::ControlStackOverflow);
        }
        self.frame_mut().control_stack.push(label);
        Ok(())
    }

    pub(crate) fn push_structured(
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

    pub(crate) fn enter_frame(
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

    pub(crate) fn exec_br(&mut self, label_depth: u32) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn exec_call(&mut self, function_index: u32) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn exec_host_invoke(
        &mut self,
        host_import_index: u32,
    ) -> Result<ExecAction, TrapKind> {
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
                    self.emit(
                        JournalEventKind::InvalidCapabilityUse {
                            table_index: h.table_index,
                            generation: h.generation,
                        },
                        Label::Public,
                    );
                    return Err(TrapKind::InvalidCapability);
                }
                capabilities.push(h);
            }
        }
        if !self.plugins.contains(&(plugin_id, method_id)) {
            return Err(TrapKind::HostNotFound);
        }
        self.frame_mut().instruction_index += 1;
        let sensitivity = join_labels(&arguments);
        let journal_args: Vec<JournalValue> = arguments
            .iter()
            .map(|v| JournalValue::from_value(v, false))
            .collect();
        let continuation_identifier = self
            .machine
            .active_continuation_identifier
            .expect("active continuation");
        self.machine.pending_host_calls.push(HostCall {
            plugin_id,
            method_id,
            arguments,
            capabilities,
            continuation_identifier,
        });
        self.emit(
            JournalEventKind::HostCallSuspended {
                plugin_id,
                method_id,
                continuation_identifier,
                arity: journal_args.len() as u32,
                arguments: journal_args,
            },
            sensitivity,
        );
        let _ = self.consume_quota(QuotaDimension::HostCallCount, 1);
        Ok(ExecAction::Suspend)
    }

    pub(crate) fn bin_i32(
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

    pub(crate) fn bin_i64(
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

    pub(crate) fn cmp_i32(&mut self, op: fn(i32, i32) -> bool) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn cmp_i64(&mut self, op: fn(i64, i64) -> bool) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn un_i32_test(&mut self, op: fn(i32) -> bool) -> Result<ExecAction, TrapKind> {
        let v = self.pop_i32()?;
        let bit = if op(v.as_i32().unwrap()) { 1 } else { 0 };
        self.push(Value::i32(bit, v.label))?;
        Ok(ExecAction::Next)
    }

    pub(crate) fn un_i64_test(&mut self, op: fn(i64) -> bool) -> Result<ExecAction, TrapKind> {
        let v = self.pop_i64()?;
        let bit = if op(v.as_i64().unwrap()) { 1 } else { 0 };
        self.push(Value::i32(bit, v.label))?;
        Ok(ExecAction::Next)
    }

    pub(crate) fn exec_end(&mut self) -> Result<ExecAction, TrapKind> {
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

    pub(crate) fn exec_return(&mut self) -> Result<ExecAction, TrapKind> {
        let n = self.current_type().results.len() as u32;
        self.do_return(n)
    }

    pub(crate) fn do_return(&mut self, result_count: u32) -> Result<ExecAction, TrapKind> {
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
