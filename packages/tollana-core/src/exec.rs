use crate::decode::{encode_binary, ExportKind, FunctionType, GlobalInit, Module};
use crate::instruction::Instruction;
use crate::machine::{
    CallFrame, Continuation, ControlLabel, ControlLabelKind, MachineState, ProgramCounter,
    SuspendReason, TrapKind,
};
use crate::validate::validate;
use crate::value::{Label, Value};
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
        self.machine.remaining_fuel = initial_fuel;
        self.machine.pending_host_call = None;
        let mut locals = arguments.to_vec();
        for t in &func.locals {
            locals.push(Value::default_for(*t));
        }
        let end_index = func.instructions.len().saturating_sub(1) as u32;
        let label = ControlLabel {
            label_kind: ControlLabelKind::Block,
            parameter_count: ty.parameters.len() as u32,
            result_count: ty.results.len() as u32,
            stack_height: 0,
            branch_instruction_index: end_index,
        };
        if self.control_stack_cap == 0 {
            return Ok(self.trap(
                TrapKind::ControlStackOverflow,
                ProgramCounter {
                    function_index,
                    instruction_index: 0,
                },
            ));
        }
        if self.call_frame_cap == 0 {
            return Ok(self.trap(
                TrapKind::CallStackOverflow,
                ProgramCounter {
                    function_index,
                    instruction_index: 0,
                },
            ));
        }
        let frame = CallFrame {
            function_index,
            instruction_index: 0,
            locals,
            control_stack: vec![label],
            return_program_counter: None,
        };
        self.machine.continuations = vec![Continuation {
            continuation_identifier: 0,
            value_stack: Vec::new(),
            call_frames: vec![frame],
        }];
        self.machine.active_continuation_identifier = Some(0);
        self.last_outcome = None;
        self.continue_run()
    }

    pub fn add_fuel(&mut self, amount: u64) {
        self.machine.remaining_fuel = self.machine.remaining_fuel.saturating_add(amount);
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
            Instruction::I32Load { .. }
            | Instruction::I32Store { .. }
            | Instruction::I64Load { .. }
            | Instruction::I64Store { .. }
            | Instruction::MemorySize
            | Instruction::Block { .. }
            | Instruction::Loop { .. }
            | Instruction::If { .. }
            | Instruction::Else
            | Instruction::Br { .. }
            | Instruction::BrIf { .. }
            | Instruction::Call { .. }
            | Instruction::HostInvoke { .. } => Err(TrapKind::TypeMismatch),
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
    use crate::value::Label;

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
}
