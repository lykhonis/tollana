use super::*;
use crate::decode::FunctionType;
use crate::instruction::Instruction;
use crate::journal::{join_labels, JournalEventKind, JournalSink, JournalValue};
use crate::machine::{
    CallFrame, Continuation, ProgramCounter, QuotaDimension, SuspendReason, TrapKind,
};
use crate::value::{Label, Value};

impl Instance {
    pub fn continue_run(&mut self) -> Result<ExecOutcome, HostInterfaceError> {
        if matches!(self.last_outcome, Some(ExecOutcome::Trapped { .. })) {
            return Ok(self.last_outcome.clone().unwrap());
        }
        match self.lowest_ready_id() {
            Some(id) => {
                self.machine.active_continuation_identifier = Some(id);
            }
            None => {
                if !self.machine.pending_host_calls.is_empty() {
                    return Err(HostInterfaceError::HostCallPending);
                }
                return match &self.last_outcome {
                    Some(ExecOutcome::Completed { .. }) => Ok(self.last_outcome.clone().unwrap()),
                    _ => Err(HostInterfaceError::InstanceIdle),
                };
            }
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

    pub(crate) fn emit(&mut self, kind: JournalEventKind, sensitivity: Label) {
        self.journal.append(kind, sensitivity);
    }

    pub(crate) fn trap(&mut self, kind: TrapKind, pc: ProgramCounter) -> ExecOutcome {
        self.machine.active_continuation_identifier = None;
        self.emit(
            JournalEventKind::Trapped {
                trap_kind: kind,
                program_counter: pc,
            },
            Label::Public,
        );
        let outcome = ExecOutcome::Trapped {
            trap_kind: kind,
            program_counter: pc,
        };
        self.last_outcome = Some(outcome.clone());
        outcome
    }

    pub(crate) fn pc(&self) -> ProgramCounter {
        let frame = self.frame();
        ProgramCounter {
            function_index: frame.function_index,
            instruction_index: frame.instruction_index,
        }
    }

    pub(crate) fn continuation(&self) -> &Continuation {
        let idx = self.active_index();
        &self.machine.continuations[idx]
    }

    pub(crate) fn continuation_mut(&mut self) -> &mut Continuation {
        let idx = self.active_index();
        &mut self.machine.continuations[idx]
    }

    pub(crate) fn active_index(&self) -> usize {
        let id = self
            .machine
            .active_continuation_identifier
            .expect("active continuation");
        self.machine
            .continuations
            .iter()
            .position(|c| c.continuation_identifier == id)
            .expect("active continuation present")
    }

    pub(crate) fn lowest_ready_id(&self) -> Option<u32> {
        self.machine
            .continuations
            .iter()
            .filter(|c| self.is_ready(c))
            .map(|c| c.continuation_identifier)
            .min()
    }

    pub(crate) fn is_ready(&self, continuation: &Continuation) -> bool {
        !continuation.call_frames.is_empty()
            && !self
                .machine
                .pending_host_calls
                .iter()
                .any(|c| c.continuation_identifier == continuation.continuation_identifier)
    }

    pub(crate) fn frame(&self) -> &CallFrame {
        self.continuation().call_frames.last().unwrap()
    }

    pub(crate) fn frame_mut(&mut self) -> &mut CallFrame {
        self.continuation_mut().call_frames.last_mut().unwrap()
    }

    pub(crate) fn instructions(&self) -> &[Instruction] {
        let idx = self.frame().function_index as usize;
        &self.module.functions[idx].instructions
    }

    pub(crate) fn current_type(&self) -> &FunctionType {
        let f = &self.module.functions[self.frame().function_index as usize];
        &self.module.types[f.type_index as usize]
    }

    pub(crate) fn push(&mut self, v: Value) -> Result<(), TrapKind> {
        if self.continuation().value_stack.len() >= self.value_stack_cap {
            return Err(TrapKind::ValueStackOverflow);
        }
        self.continuation_mut().value_stack.push(v);
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Result<Value, TrapKind> {
        self.continuation_mut()
            .value_stack
            .pop()
            .ok_or(TrapKind::ValueStackUnderflow)
    }

    pub(crate) fn pop_i32(&mut self) -> Result<Value, TrapKind> {
        let v = self.pop()?;
        if v.as_i32().is_none() {
            return Err(TrapKind::TypeMismatch);
        }
        Ok(v)
    }

    pub(crate) fn pop_i64(&mut self) -> Result<Value, TrapKind> {
        let v = self.pop()?;
        if v.as_i64().is_none() {
            return Err(TrapKind::TypeMismatch);
        }
        Ok(v)
    }

    pub(crate) fn step(&mut self) -> Step {
        let pc = self.pc();
        let inst = {
            let insts = self.instructions();
            if pc.instruction_index as usize >= insts.len() {
                return Step::Done(self.trap(TrapKind::InvalidProgramCounter, pc));
            }
            insts[pc.instruction_index as usize]
        };
        if self.machine.remaining_fuel == 0 {
            self.emit(
                JournalEventKind::FuelSuspended {
                    remaining_fuel: 0,
                    program_counter: pc,
                },
                Label::Public,
            );
            return Step::Done(ExecOutcome::Suspended {
                reason: SuspendReason::OutOfFuel,
            });
        }
        if matches!(inst, Instruction::HostInvoke { .. })
            && self.quota_remaining(QuotaDimension::HostCallCount) == Some(0)
        {
            self.emit(
                JournalEventKind::QuotaExhausted {
                    dimension: QuotaDimension::HostCallCount,
                    program_counter: pc,
                },
                Label::Public,
            );
            return Step::Done(ExecOutcome::Suspended {
                reason: SuspendReason::QuotaExhausted {
                    dimension: QuotaDimension::HostCallCount,
                },
            });
        }
        self.machine.remaining_fuel -= 1;
        if self.journal.emit_instruction_stepped {
            self.emit(
                JournalEventKind::InstructionStepped {
                    function_index: pc.function_index,
                    instruction_index: pc.instruction_index,
                    opcode: inst.name(),
                },
                Label::Public,
            );
        }
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
                let id = self.machine.active_continuation_identifier;
                self.machine
                    .continuations
                    .retain(|c| Some(c.continuation_identifier) != id);
                self.machine.active_continuation_identifier = None;
                let sensitivity = join_labels(&results);
                let journal_results: Vec<JournalValue> = results
                    .iter()
                    .map(|v| JournalValue::from_value(v, false))
                    .collect();
                self.emit(
                    JournalEventKind::Completed {
                        results: journal_results,
                    },
                    sensitivity,
                );
                Step::Done(ExecOutcome::Completed { results })
            }
            Err(kind) => Step::Done(self.trap(kind, pc)),
        }
    }
}
