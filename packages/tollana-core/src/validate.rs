use crate::decode::{ExportKind, Function, FunctionType, GlobalInit, Module, Mutability};
use crate::instruction::{BlockType, Instruction};
use crate::value::ValueType;
use std::collections::HashSet;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidateError {
    pub message: String,
}

impl ValidateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ValidateError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Function,
    Block,
    Loop,
    If,
}

struct Frame {
    kind: FrameKind,
    params: Vec<ValueType>,
    results: Vec<ValueType>,
    height: usize,
    unreachable: bool,
    else_seen: bool,
}

struct FuncValidator<'a> {
    module: &'a Module,
    locals: Vec<ValueType>,
    type_stack: Vec<ValueType>,
    frames: Vec<Frame>,
}

impl<'a> FuncValidator<'a> {
    fn current(&self) -> &Frame {
        self.frames.last().expect("control stack")
    }

    fn current_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("control stack")
    }

    fn function_results(&self) -> &[ValueType] {
        &self.frames[0].results
    }

    fn push_type(&mut self, t: ValueType) {
        self.type_stack.push(t);
    }

    fn push_types(&mut self, ts: &[ValueType]) {
        self.type_stack.extend_from_slice(ts);
    }

    fn pop_type(&mut self, expected: ValueType) -> Result<(), ValidateError> {
        let height = self.current().height;
        let unreachable = self.current().unreachable;
        if unreachable && self.type_stack.len() == height {
            return Ok(());
        }
        if self.type_stack.len() <= height {
            return Err(ValidateError::new("type stack underflow"));
        }
        let actual = self.type_stack.pop().unwrap();
        if actual != expected {
            return Err(ValidateError::new(format!(
                "type mismatch: expected {}, got {}",
                expected.name(),
                actual.name()
            )));
        }
        Ok(())
    }

    fn pop_any(&mut self) -> Result<(), ValidateError> {
        let height = self.current().height;
        let unreachable = self.current().unreachable;
        if unreachable && self.type_stack.len() == height {
            return Ok(());
        }
        if self.type_stack.len() <= height {
            return Err(ValidateError::new("type stack underflow"));
        }
        self.type_stack.pop();
        Ok(())
    }

    fn pop_types(&mut self, ts: &[ValueType]) -> Result<(), ValidateError> {
        for t in ts.iter().rev() {
            self.pop_type(*t)?;
        }
        Ok(())
    }

    fn mark_unreachable(&mut self) {
        let height = self.current().height;
        self.current_mut().unreachable = true;
        self.type_stack.truncate(height);
    }

    fn resolve_block_type(
        &self,
        bt: BlockType,
    ) -> Result<(Vec<ValueType>, Vec<ValueType>), ValidateError> {
        match bt {
            BlockType::Empty => Ok((Vec::new(), Vec::new())),
            BlockType::SingleResult(t) => Ok((Vec::new(), vec![t])),
            BlockType::TypeIndex(i) => {
                let ty = self
                    .module
                    .types
                    .get(i as usize)
                    .ok_or_else(|| ValidateError::new("typeIndex out of range"))?;
                Ok((ty.parameters.clone(), ty.results.clone()))
            }
        }
    }

    fn push_ctrl(
        &mut self,
        kind: FrameKind,
        params: Vec<ValueType>,
        results: Vec<ValueType>,
    ) -> Result<(), ValidateError> {
        self.pop_types(&params)?;
        let height = self.type_stack.len();
        self.frames.push(Frame {
            kind,
            params: params.clone(),
            results,
            height,
            unreachable: false,
            else_seen: false,
        });
        self.push_types(&params);
        Ok(())
    }

    fn branch_sig(&self, label_depth: u32) -> Result<Vec<ValueType>, ValidateError> {
        let idx = self
            .frames
            .len()
            .checked_sub(1 + label_depth as usize)
            .ok_or_else(|| ValidateError::new("labelDepth out of range"))?;
        let frame = &self.frames[idx];
        if frame.kind == FrameKind::Loop {
            Ok(frame.params.clone())
        } else {
            Ok(frame.results.clone())
        }
    }

    fn require_memory(&self) -> Result<(), ValidateError> {
        if self.module.memory_page_count.is_none() {
            return Err(ValidateError::new("memory instruction without memory"));
        }
        Ok(())
    }

    fn local_type(&self, index: u32) -> Result<ValueType, ValidateError> {
        self.locals
            .get(index as usize)
            .copied()
            .ok_or_else(|| ValidateError::new("localIndex out of range"))
    }

    fn global(&self, index: u32) -> Result<(ValueType, Mutability), ValidateError> {
        self.module
            .globals
            .get(index as usize)
            .map(|g| (g.value_type, g.mutability))
            .ok_or_else(|| ValidateError::new("globalIndex out of range"))
    }

    fn function_type(&self, index: u32) -> Result<&FunctionType, ValidateError> {
        let f = self
            .module
            .functions
            .get(index as usize)
            .ok_or_else(|| ValidateError::new("functionIndex out of range"))?;
        self.module
            .types
            .get(f.type_index as usize)
            .ok_or_else(|| ValidateError::new("typeIndex out of range"))
    }

    fn import_type(&self, index: u32) -> Result<&FunctionType, ValidateError> {
        let imp = self
            .module
            .host_imports
            .get(index as usize)
            .ok_or_else(|| ValidateError::new("hostImportIndex out of range"))?;
        self.module
            .types
            .get(imp.type_index as usize)
            .ok_or_else(|| ValidateError::new("typeIndex out of range"))
    }

    fn step(&mut self, inst: Instruction) -> Result<(), ValidateError> {
        if self.frames.is_empty() {
            return Err(ValidateError::new("instruction after function end"));
        }
        match inst {
            Instruction::Nop => {}
            Instruction::Unreachable => self.mark_unreachable(),
            Instruction::Drop => self.pop_any()?,
            Instruction::I32Const { .. } => self.push_type(ValueType::I32),
            Instruction::I64Const { .. } => self.push_type(ValueType::I64),
            Instruction::I32Add
            | Instruction::I32Sub
            | Instruction::I32Mul
            | Instruction::I32DivS
            | Instruction::I32RemS
            | Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtS
            | Instruction::I32GtS
            | Instruction::I32LeS
            | Instruction::I32GeS => {
                self.pop_types(&[ValueType::I32, ValueType::I32])?;
                self.push_type(ValueType::I32);
            }
            Instruction::I32Eqz => {
                self.pop_type(ValueType::I32)?;
                self.push_type(ValueType::I32);
            }
            Instruction::I64Add
            | Instruction::I64Sub
            | Instruction::I64Mul
            | Instruction::I64DivS
            | Instruction::I64RemS => {
                self.pop_types(&[ValueType::I64, ValueType::I64])?;
                self.push_type(ValueType::I64);
            }
            Instruction::I64Eqz => {
                self.pop_type(ValueType::I64)?;
                self.push_type(ValueType::I32);
            }
            Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtS
            | Instruction::I64GtS
            | Instruction::I64LeS
            | Instruction::I64GeS => {
                self.pop_types(&[ValueType::I64, ValueType::I64])?;
                self.push_type(ValueType::I32);
            }
            Instruction::LocalGet { local_index } => {
                let t = self.local_type(local_index)?;
                self.push_type(t);
            }
            Instruction::LocalSet { local_index } => {
                let t = self.local_type(local_index)?;
                self.pop_type(t)?;
            }
            Instruction::LocalTee { local_index } => {
                let t = self.local_type(local_index)?;
                self.pop_type(t)?;
                self.push_type(t);
            }
            Instruction::GlobalGet { global_index } => {
                let (t, _) = self.global(global_index)?;
                self.push_type(t);
            }
            Instruction::GlobalSet { global_index } => {
                let (t, mutability) = self.global(global_index)?;
                if mutability != Mutability::Mutable {
                    return Err(ValidateError::new("global.set on immutable global"));
                }
                self.pop_type(t)?;
            }
            Instruction::I32Load { .. } => {
                self.require_memory()?;
                self.pop_type(ValueType::I32)?;
                self.push_type(ValueType::I32);
            }
            Instruction::I32Store { .. } => {
                self.require_memory()?;
                self.pop_types(&[ValueType::I32, ValueType::I32])?;
            }
            Instruction::I64Load { .. } => {
                self.require_memory()?;
                self.pop_type(ValueType::I32)?;
                self.push_type(ValueType::I64);
            }
            Instruction::I64Store { .. } => {
                self.require_memory()?;
                self.pop_type(ValueType::I64)?;
                self.pop_type(ValueType::I32)?;
            }
            Instruction::MemorySize => {
                self.require_memory()?;
                self.push_type(ValueType::I32);
            }
            Instruction::Block { block_type } => {
                let (params, results) = self.resolve_block_type(block_type)?;
                self.push_ctrl(FrameKind::Block, params, results)?;
            }
            Instruction::Loop { block_type } => {
                let (params, results) = self.resolve_block_type(block_type)?;
                self.push_ctrl(FrameKind::Loop, params, results)?;
            }
            Instruction::If { block_type } => {
                self.pop_type(ValueType::I32)?;
                let (params, results) = self.resolve_block_type(block_type)?;
                self.push_ctrl(FrameKind::If, params, results)?;
            }
            Instruction::Else => {
                if self.current().kind != FrameKind::If || self.current().else_seen {
                    return Err(ValidateError::new("else without matching if"));
                }
                let results = self.current().results.clone();
                let params = self.current().params.clone();
                let height = self.current().height;
                let unreachable = self.current().unreachable;
                self.pop_types(&results)?;
                if !unreachable && self.type_stack.len() != height {
                    return Err(ValidateError::new("else type stack height"));
                }
                self.type_stack.truncate(height);
                let frame = self.current_mut();
                frame.else_seen = true;
                frame.unreachable = false;
                self.push_types(&params);
            }
            Instruction::End => {
                let results = self.current().results.clone();
                let height = self.current().height;
                let unreachable = self.current().unreachable;
                let kind = self.current().kind;
                let else_seen = self.current().else_seen;
                self.pop_types(&results)?;
                if !unreachable && self.type_stack.len() != height {
                    return Err(ValidateError::new("end type stack height"));
                }
                self.type_stack.truncate(height);
                self.frames.pop();
                if kind == FrameKind::If && !results.is_empty() && !else_seen {
                    return Err(ValidateError::new("if with results requires else"));
                }
                self.push_types(&results);
                if kind == FrameKind::Function {
                    if !self.frames.is_empty() {
                        return Err(ValidateError::new("function end with nested frames"));
                    }
                    if self.type_stack != results {
                        return Err(ValidateError::new("function end result types"));
                    }
                }
            }
            Instruction::Br { label_depth } => {
                let sig = self.branch_sig(label_depth)?;
                self.pop_types(&sig)?;
                self.mark_unreachable();
            }
            Instruction::BrIf { label_depth } => {
                self.pop_type(ValueType::I32)?;
                let sig = self.branch_sig(label_depth)?;
                self.pop_types(&sig)?;
                self.push_types(&sig);
            }
            Instruction::Call { function_index } => {
                let ty = self.function_type(function_index)?.clone();
                self.pop_types(&ty.parameters)?;
                self.push_types(&ty.results);
            }
            Instruction::Return => {
                let results = self.function_results().to_vec();
                self.pop_types(&results)?;
                self.mark_unreachable();
            }
            Instruction::HostInvoke { host_import_index } => {
                let ty = self.import_type(host_import_index)?.clone();
                self.pop_types(&ty.parameters)?;
                self.push_types(&ty.results);
            }
        }
        Ok(())
    }
}

fn validate_function(module: &Module, func: &Function) -> Result<(), ValidateError> {
    let ty = module
        .types
        .get(func.type_index as usize)
        .ok_or_else(|| ValidateError::new("typeIndex out of range"))?;
    let mut locals = ty.parameters.clone();
    locals.extend_from_slice(&func.locals);
    if locals.len() > 4096 {
        return Err(ValidateError::new("too many locals"));
    }
    let mut v = FuncValidator {
        module,
        locals,
        type_stack: Vec::new(),
        frames: vec![Frame {
            kind: FrameKind::Function,
            params: ty.parameters.clone(),
            results: ty.results.clone(),
            height: 0,
            unreachable: false,
            else_seen: false,
        }],
    };
    for inst in &func.instructions {
        v.step(*inst)?;
    }
    if !v.frames.is_empty() {
        return Err(ValidateError::new("function missing end"));
    }
    Ok(())
}

fn validate_global_init(module: &Module) -> Result<(), ValidateError> {
    for g in &module.globals {
        match &g.init {
            GlobalInit::HostInjected => {}
            GlobalInit::ConstantExpression(insts) => {
                if g.value_type == ValueType::Capability {
                    return Err(ValidateError::new(
                        "Capability global must be host.injected",
                    ));
                }
                match insts.as_slice() {
                    [Instruction::I32Const { .. }, Instruction::End]
                        if g.value_type == ValueType::I32 => {}
                    [Instruction::I64Const { .. }, Instruction::End]
                        if g.value_type == ValueType::I64 => {}
                    _ => {
                        return Err(ValidateError::new("invalid global constant expression"));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn validate(module: &Module) -> Result<(), ValidateError> {
    for imp in &module.host_imports {
        if module.types.get(imp.type_index as usize).is_none() {
            return Err(ValidateError::new("host import typeIndex out of range"));
        }
    }
    for func in &module.functions {
        if module.types.get(func.type_index as usize).is_none() {
            return Err(ValidateError::new("function typeIndex out of range"));
        }
    }

    let mut export_names = HashSet::new();
    for e in &module.exports {
        if !export_names.insert(&e.name) {
            return Err(ValidateError::new("duplicate export name"));
        }
        match e.kind {
            ExportKind::Function => {
                if module.functions.get(e.index as usize).is_none() {
                    return Err(ValidateError::new("export functionIndex out of range"));
                }
            }
            ExportKind::Memory => {
                if module.memory_page_count.is_none() || e.index != 0 {
                    return Err(ValidateError::new("export memoryIndex out of range"));
                }
            }
            ExportKind::Global => {
                if module.globals.get(e.index as usize).is_none() {
                    return Err(ValidateError::new("export globalIndex out of range"));
                }
            }
        }
    }

    let mut import_names = HashSet::new();
    let mut import_pairs = HashSet::new();
    for imp in &module.host_imports {
        if !import_names.insert(&imp.name) {
            return Err(ValidateError::new("duplicate host.import name"));
        }
        if !import_pairs.insert((imp.plugin_id, imp.method_id)) {
            return Err(ValidateError::new("duplicate pluginId methodId"));
        }
    }

    validate_global_init(module)?;

    for func in &module.functions {
        validate_function(module, func)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_text;

    fn ok(src: &str) {
        let m = decode_text(src).expect("decode");
        validate(&m).expect("validate");
    }

    fn err(src: &str) {
        let m = decode_text(src).expect("decode");
        assert!(validate(&m).is_err(), "expected validation error");
    }

    #[test]
    fn add_program_is_valid() {
        ok(r#"
(module
  (func (export "main") (result i32)
    (i32.add (i32.const 1) (i32.const 2))))
"#);
    }

    #[test]
    fn echo_program_is_valid() {
        ok(r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#);
    }

    #[test]
    fn program_7_integer_as_capability_is_rejected() {
        err(r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke UseCapability (i32.const 1))))
"#);
    }

    #[test]
    fn program_7_positive_is_valid() {
        ok(r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke UseCapability (local.get 0))))
"#);
    }

    #[test]
    fn i32_add_type_mismatch_is_rejected() {
        err(r#"
(module
  (func (export "main") (result i32)
    (i32.add (i64.const 1) (i32.const 2))))
"#);
    }

    #[test]
    fn load_without_memory_is_rejected() {
        err(r#"
(module
  (func (export "main") (result i32)
    (i32.load (i32.const 0))))
"#);
    }

    #[test]
    fn extra_stack_value_at_end_is_rejected() {
        err(r#"
(module
  (func (export "main") (result i32)
    i32.const 1
    i32.const 2))
"#);
    }

    #[test]
    fn if_with_result_requires_else() {
        err(r#"
(module
  (func (export "main") (result i32)
    (if (result i32) (i32.const 1) (i32.const 2))))
"#);
    }
}
