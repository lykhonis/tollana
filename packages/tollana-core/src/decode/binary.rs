use super::error::DecodeError;
use super::module::{
    Export, ExportKind, Function, FunctionType, Global, GlobalInit, HostImport, Module, Mutability,
};
use crate::instruction::{BlockType, Instruction};
use crate::value::ValueType;

const MAGIC: &[u8; 4] = b"TIR\0";
const FORMAT_VERSION: u16 = 1;
const MAX_MODULE_BYTES: usize = 16_777_216;
const MAX_FUNCTIONS: u32 = 4096;
const MAX_PAGES: u32 = 65_536;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        if self.pos >= self.data.len() {
            return Err(DecodeError::new("unexpected end of binary"));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        let lo = self.u8()? as u16;
        let hi = self.u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b0 = self.u8()? as u32;
        let b1 = self.u8()? as u32;
        let b2 = self.u8()? as u32;
        let b3 = self.u8()? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(self.u32()? as i32)
    }

    fn i64(&mut self) -> Result<i64, DecodeError> {
        let lo = self.u32()? as u64;
        let hi = self.u32()? as u64;
        Ok((lo | (hi << 32)) as i64)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < n {
            return Err(DecodeError::new("unexpected end of binary"));
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }

    fn name(&mut self) -> Result<String, DecodeError> {
        let len = self.u32()? as usize;
        let bytes = self.bytes(len)?;
        let s = std::str::from_utf8(bytes).map_err(|_| DecodeError::new("invalid UTF-8 name"))?;
        if s.contains('\0') {
            return Err(DecodeError::new("U+0000 in name"));
        }
        Ok(s.to_string())
    }

    fn value_type(&mut self) -> Result<ValueType, DecodeError> {
        let code = self.u8()?;
        ValueType::from_code(code)
            .ok_or_else(|| DecodeError::new(format!("unknown value type {code:#04x}")))
    }
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }

    fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn name(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    fn value_type(&mut self, t: ValueType) {
        self.u8(t.code());
    }

    fn section(&mut self, id: u8, payload: &[u8]) {
        self.u8(id);
        self.u32(payload.len() as u32);
        self.buf.extend_from_slice(payload);
    }
}

fn read_block_type(r: &mut Reader<'_>) -> Result<BlockType, DecodeError> {
    match r.u8()? {
        0x00 => Ok(BlockType::Empty),
        0x01 => Ok(BlockType::SingleResult(r.value_type()?)),
        0x02 => Ok(BlockType::TypeIndex(r.u32()?)),
        other => Err(DecodeError::new(format!("unknown block type {other:#04x}"))),
    }
}

fn write_block_type(w: &mut Writer, bt: BlockType) {
    match bt {
        BlockType::Empty => w.u8(0x00),
        BlockType::SingleResult(t) => {
            w.u8(0x01);
            w.value_type(t);
        }
        BlockType::TypeIndex(i) => {
            w.u8(0x02);
            w.u32(i);
        }
    }
}

fn read_instruction(r: &mut Reader<'_>) -> Result<Instruction, DecodeError> {
    let op = r.u8()?;
    match op {
        0x00 => Ok(Instruction::Nop),
        0x01 => Ok(Instruction::Unreachable),
        0x02 => Ok(Instruction::Drop),
        0x10 => Ok(Instruction::I32Const { value: r.i32()? }),
        0x11 => Ok(Instruction::I64Const { value: r.i64()? }),
        0x20 => Ok(Instruction::I32Add),
        0x21 => Ok(Instruction::I32Sub),
        0x22 => Ok(Instruction::I32Mul),
        0x23 => Ok(Instruction::I32DivS),
        0x24 => Ok(Instruction::I32RemS),
        0x25 => Ok(Instruction::I32Eqz),
        0x26 => Ok(Instruction::I32Eq),
        0x27 => Ok(Instruction::I32Ne),
        0x28 => Ok(Instruction::I32LtS),
        0x29 => Ok(Instruction::I32GtS),
        0x2A => Ok(Instruction::I32LeS),
        0x2B => Ok(Instruction::I32GeS),
        0x30 => Ok(Instruction::I64Add),
        0x31 => Ok(Instruction::I64Sub),
        0x32 => Ok(Instruction::I64Mul),
        0x33 => Ok(Instruction::I64DivS),
        0x34 => Ok(Instruction::I64RemS),
        0x35 => Ok(Instruction::I64Eqz),
        0x36 => Ok(Instruction::I64Eq),
        0x37 => Ok(Instruction::I64Ne),
        0x38 => Ok(Instruction::I64LtS),
        0x39 => Ok(Instruction::I64GtS),
        0x3A => Ok(Instruction::I64LeS),
        0x3B => Ok(Instruction::I64GeS),
        0x40 => Ok(Instruction::LocalGet {
            local_index: r.u32()?,
        }),
        0x41 => Ok(Instruction::LocalSet {
            local_index: r.u32()?,
        }),
        0x42 => Ok(Instruction::LocalTee {
            local_index: r.u32()?,
        }),
        0x43 => Ok(Instruction::GlobalGet {
            global_index: r.u32()?,
        }),
        0x44 => Ok(Instruction::GlobalSet {
            global_index: r.u32()?,
        }),
        0x50 => Ok(Instruction::I32Load {
            immediate_offset: r.u32()?,
        }),
        0x51 => Ok(Instruction::I32Store {
            immediate_offset: r.u32()?,
        }),
        0x52 => Ok(Instruction::I64Load {
            immediate_offset: r.u32()?,
        }),
        0x53 => Ok(Instruction::I64Store {
            immediate_offset: r.u32()?,
        }),
        0x54 => Ok(Instruction::MemorySize),
        0x60 => Ok(Instruction::Block {
            block_type: read_block_type(r)?,
        }),
        0x61 => Ok(Instruction::Loop {
            block_type: read_block_type(r)?,
        }),
        0x62 => Ok(Instruction::If {
            block_type: read_block_type(r)?,
        }),
        0x63 => Ok(Instruction::Else),
        0x64 => Ok(Instruction::End),
        0x65 => Ok(Instruction::Br {
            label_depth: r.u32()?,
        }),
        0x66 => Ok(Instruction::BrIf {
            label_depth: r.u32()?,
        }),
        0x67 => Ok(Instruction::Call {
            function_index: r.u32()?,
        }),
        0x68 => Ok(Instruction::Return),
        0x70 => Ok(Instruction::HostInvoke {
            host_import_index: r.u32()?,
        }),
        other => Err(DecodeError::new(format!("unknown opcode {other:#04x}"))),
    }
}

fn write_instruction(w: &mut Writer, inst: Instruction) {
    w.u8(inst.opcode());
    match inst {
        Instruction::I32Const { value } => w.i32(value),
        Instruction::I64Const { value } => w.i64(value),
        Instruction::LocalGet { local_index } => w.u32(local_index),
        Instruction::LocalSet { local_index } => w.u32(local_index),
        Instruction::LocalTee { local_index } => w.u32(local_index),
        Instruction::GlobalGet { global_index } => w.u32(global_index),
        Instruction::GlobalSet { global_index } => w.u32(global_index),
        Instruction::I32Load { immediate_offset }
        | Instruction::I32Store { immediate_offset }
        | Instruction::I64Load { immediate_offset }
        | Instruction::I64Store { immediate_offset } => w.u32(immediate_offset),
        Instruction::Block { block_type }
        | Instruction::Loop { block_type }
        | Instruction::If { block_type } => write_block_type(w, block_type),
        Instruction::Br { label_depth } | Instruction::BrIf { label_depth } => w.u32(label_depth),
        Instruction::Call { function_index } => w.u32(function_index),
        Instruction::HostInvoke { host_import_index } => w.u32(host_import_index),
        _ => {}
    }
}

fn read_instruction_list(r: &mut Reader<'_>) -> Result<Vec<Instruction>, DecodeError> {
    let mut out = Vec::new();
    loop {
        if r.is_empty() {
            return Err(DecodeError::new("instruction list missing end"));
        }
        let inst = read_instruction(r)?;
        let is_end = inst == Instruction::End;
        out.push(inst);
        if is_end {
            return Ok(out);
        }
    }
}

pub fn decode_binary(data: &[u8]) -> Result<Module, DecodeError> {
    if data.len() > MAX_MODULE_BYTES {
        return Err(DecodeError::new("module exceeds 16 MiB"));
    }
    let mut r = Reader::new(data);
    if r.bytes(4)? != MAGIC {
        return Err(DecodeError::new("bad magic"));
    }
    if r.u16()? != FORMAT_VERSION {
        return Err(DecodeError::new("unsupported formatVersion"));
    }
    if r.u16()? != 0 {
        return Err(DecodeError::new("nonzero reserved header field"));
    }

    let mut module = Module::new();
    let mut last_required = 0u8;
    let mut saw_code = false;
    let mut function_types: Vec<u32> = Vec::new();

    while !r.is_empty() {
        let id = r.u8()?;
        let len = r.u32()? as usize;
        if len > r.remaining() {
            return Err(DecodeError::new(
                "sectionByteLength exceeds remaining bytes",
            ));
        }
        let payload = r.bytes(len)?;
        let mut s = Reader::new(payload);
        match id {
            0x00 => {
                let _name = s.name()?;
            }
            0x01 => {
                if last_required >= 1 {
                    return Err(DecodeError::new("type section out of order or duplicate"));
                }
                last_required = 1;
                let n = s.u32()?;
                if n > MAX_FUNCTIONS {
                    return Err(DecodeError::new("too many function types"));
                }
                for _ in 0..n {
                    let pc = s.u32()? as usize;
                    let mut parameters = Vec::with_capacity(pc);
                    for _ in 0..pc {
                        parameters.push(s.value_type()?);
                    }
                    let rc = s.u32()? as usize;
                    let mut results = Vec::with_capacity(rc);
                    for _ in 0..rc {
                        results.push(s.value_type()?);
                    }
                    module.types.push(FunctionType {
                        parameters,
                        results,
                    });
                }
            }
            0x02 => {
                if last_required > 2 {
                    return Err(DecodeError::new("host import section out of order"));
                }
                if last_required == 2 {
                    return Err(DecodeError::new("duplicate host import section"));
                }
                last_required = 2;
                let n = s.u32()?;
                for _ in 0..n {
                    module.host_imports.push(HostImport {
                        name: s.name()?,
                        plugin_id: s.u32()?,
                        method_id: s.u32()?,
                        type_index: s.u32()?,
                    });
                }
            }
            0x03 => {
                if last_required > 3 {
                    return Err(DecodeError::new("function section out of order"));
                }
                if last_required == 3 {
                    return Err(DecodeError::new("duplicate function section"));
                }
                last_required = 3;
                let n = s.u32()?;
                if n > MAX_FUNCTIONS {
                    return Err(DecodeError::new("too many functions"));
                }
                for _ in 0..n {
                    function_types.push(s.u32()?);
                }
            }
            0x04 => {
                if last_required > 4 {
                    return Err(DecodeError::new("memory section out of order"));
                }
                if last_required == 4 {
                    return Err(DecodeError::new("duplicate memory section"));
                }
                last_required = 4;
                let n = s.u32()?;
                if n > 1 {
                    return Err(DecodeError::new("memoryCount must be 0 or 1"));
                }
                if n == 1 {
                    let pages = s.u32()?;
                    if pages > MAX_PAGES {
                        return Err(DecodeError::new("pageCount exceeds 65536"));
                    }
                    module.memory_page_count = Some(pages);
                }
            }
            0x05 => {
                if last_required > 5 {
                    return Err(DecodeError::new("global section out of order"));
                }
                if last_required == 5 {
                    return Err(DecodeError::new("duplicate global section"));
                }
                last_required = 5;
                let n = s.u32()?;
                for _ in 0..n {
                    let value_type = s.value_type()?;
                    let mutability = match s.u8()? {
                        0 => Mutability::Immutable,
                        1 => Mutability::Mutable,
                        other => {
                            return Err(DecodeError::new(format!("bad mutability {other}")));
                        }
                    };
                    let init = match s.u8()? {
                        0x01 => GlobalInit::ConstantExpression(read_instruction_list(&mut s)?),
                        0x02 => GlobalInit::HostInjected,
                        other => {
                            return Err(DecodeError::new(format!(
                                "bad globalInitKind {other:#04x}"
                            )));
                        }
                    };
                    module.globals.push(Global {
                        value_type,
                        mutability,
                        init,
                    });
                }
            }
            0x06 => {
                if last_required > 6 {
                    return Err(DecodeError::new("export section out of order"));
                }
                if last_required == 6 {
                    return Err(DecodeError::new("duplicate export section"));
                }
                last_required = 6;
                let n = s.u32()?;
                for _ in 0..n {
                    let name = s.name()?;
                    let kind = match s.u8()? {
                        0x01 => ExportKind::Function,
                        0x02 => ExportKind::Memory,
                        0x03 => ExportKind::Global,
                        other => {
                            return Err(DecodeError::new(format!("bad export kind {other:#04x}")));
                        }
                    };
                    module.exports.push(Export {
                        name,
                        kind,
                        index: s.u32()?,
                    });
                }
            }
            0x07 => {
                if saw_code {
                    return Err(DecodeError::new("duplicate code section"));
                }
                saw_code = true;
                last_required = 7;
                let n = s.u32()?;
                if n as usize != function_types.len() {
                    return Err(DecodeError::new(
                        "functionBodyCount must equal functionCount",
                    ));
                }
                for type_index in &function_types {
                    let body_len = s.u32()? as usize;
                    if body_len > s.remaining() {
                        return Err(DecodeError::new("bodyByteLength exceeds section"));
                    }
                    let body = s.bytes(body_len)?;
                    let mut b = Reader::new(body);
                    let group_count = b.u32()?;
                    let mut locals = Vec::new();
                    for _ in 0..group_count {
                        let count = b.u32()?;
                        let ty = b.value_type()?;
                        for _ in 0..count {
                            locals.push(ty);
                        }
                        if locals.len() > 4096 {
                            return Err(DecodeError::new("too many locals"));
                        }
                    }
                    let instructions = read_instruction_list(&mut b)?;
                    if !b.is_empty() {
                        return Err(DecodeError::new("trailing bytes in function body"));
                    }
                    module.functions.push(Function {
                        type_index: *type_index,
                        locals,
                        instructions,
                    });
                }
            }
            other => {
                return Err(DecodeError::new(format!("unknown section id {other:#04x}")));
            }
        }
        if !s.is_empty() && id != 0x00 {
            return Err(DecodeError::new("trailing bytes in section"));
        }
    }

    if function_types.len() != module.functions.len() {
        return Err(DecodeError::new("function section without matching code"));
    }
    Ok(module)
}

fn payload(write: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    write(&mut w);
    w.buf
}

pub fn encode_binary(module: &Module) -> Vec<u8> {
    let mut w = Writer::new();
    w.buf.extend_from_slice(MAGIC);
    w.u16(FORMAT_VERSION);
    w.u16(0);

    if !module.types.is_empty() {
        let p = payload(|s| {
            s.u32(module.types.len() as u32);
            for t in &module.types {
                s.u32(t.parameters.len() as u32);
                for ty in &t.parameters {
                    s.value_type(*ty);
                }
                s.u32(t.results.len() as u32);
                for ty in &t.results {
                    s.value_type(*ty);
                }
            }
        });
        w.section(0x01, &p);
    }

    if !module.host_imports.is_empty() {
        let p = payload(|s| {
            s.u32(module.host_imports.len() as u32);
            for imp in &module.host_imports {
                s.name(&imp.name);
                s.u32(imp.plugin_id);
                s.u32(imp.method_id);
                s.u32(imp.type_index);
            }
        });
        w.section(0x02, &p);
    }

    if !module.functions.is_empty() {
        let p = payload(|s| {
            s.u32(module.functions.len() as u32);
            for f in &module.functions {
                s.u32(f.type_index);
            }
        });
        w.section(0x03, &p);
    }

    if let Some(pages) = module.memory_page_count {
        let p = payload(|s| {
            s.u32(1);
            s.u32(pages);
        });
        w.section(0x04, &p);
    }

    if !module.globals.is_empty() {
        let p = payload(|s| {
            s.u32(module.globals.len() as u32);
            for g in &module.globals {
                s.value_type(g.value_type);
                s.u8(match g.mutability {
                    Mutability::Immutable => 0,
                    Mutability::Mutable => 1,
                });
                match &g.init {
                    GlobalInit::ConstantExpression(insts) => {
                        s.u8(0x01);
                        for inst in insts {
                            write_instruction(s, *inst);
                        }
                    }
                    GlobalInit::HostInjected => s.u8(0x02),
                }
            }
        });
        w.section(0x05, &p);
    }

    if !module.exports.is_empty() {
        let p = payload(|s| {
            s.u32(module.exports.len() as u32);
            for e in &module.exports {
                s.name(&e.name);
                s.u8(match e.kind {
                    ExportKind::Function => 0x01,
                    ExportKind::Memory => 0x02,
                    ExportKind::Global => 0x03,
                });
                s.u32(e.index);
            }
        });
        w.section(0x06, &p);
    }

    if !module.functions.is_empty() {
        let p = payload(|s| {
            s.u32(module.functions.len() as u32);
            for f in &module.functions {
                let body = payload(|b| {
                    let mut groups: Vec<(u32, ValueType)> = Vec::new();
                    for ty in &f.locals {
                        if let Some(last) = groups.last_mut() {
                            if last.1 == *ty {
                                last.0 += 1;
                                continue;
                            }
                        }
                        groups.push((1, *ty));
                    }
                    b.u32(groups.len() as u32);
                    for (count, ty) in groups {
                        b.u32(count);
                        b.value_type(ty);
                    }
                    for inst in &f.instructions {
                        write_instruction(b, *inst);
                    }
                });
                s.u32(body.len() as u32);
                s.buf.extend_from_slice(&body);
            }
        });
        w.section(0x07, &p);
    }

    w.buf
}
