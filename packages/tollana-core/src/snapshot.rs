use crate::machine::{
    CallFrame, CapabilityTableEntry, Continuation, ControlLabel, HostCall, ProgramCounter,
    QuotaDimension, QuotaSlot, MAX_CONTINUATIONS,
};
use crate::value::{CapHandle, Label, Value, ValuePayload, ValueType};
use std::collections::HashSet;
use std::fmt;

const MAGIC: &[u8; 4] = b"TIRS";
const FORMAT_VERSION: u16 = 1;
const MAX_BYTES: usize = 16_777_216;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotError {
    pub message: String,
}

impl SnapshotError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SnapshotError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginIdentity {
    pub plugin_id: u32,
    pub identity_hash: [u8; 32],
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRebind {
    pub plugin_id: u32,
    pub identity_hash: [u8; 32],
    pub name: String,
    pub version: String,
    pub methods: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreSnapshot {
    pub module_bytes: Vec<u8>,
    pub entry_name: String,
    pub plugin_identities: Vec<PluginIdentity>,
    pub remaining_fuel: u64,
    pub quotas: Vec<QuotaSlot>,
    pub linear_memory: Vec<u8>,
    pub globals: Vec<Value>,
    pub capability_table: Vec<CapabilityTableEntry>,
    pub active_continuation_identifier: Option<u32>,
    pub continuations: Vec<Continuation>,
    pub pending_host_calls: Vec<HostCall>,
}

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

    fn u8(&mut self) -> Result<u8, SnapshotError> {
        if self.pos >= self.data.len() {
            return Err(SnapshotError::new("unexpected end of TIRS"));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, SnapshotError> {
        let lo = self.u8()? as u16;
        let hi = self.u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn u32(&mut self) -> Result<u32, SnapshotError> {
        let b0 = self.u8()? as u32;
        let b1 = self.u8()? as u32;
        let b2 = self.u8()? as u32;
        let b3 = self.u8()? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn u64(&mut self) -> Result<u64, SnapshotError> {
        let lo = self.u32()? as u64;
        let hi = self.u32()? as u64;
        Ok(lo | (hi << 32))
    }

    fn i32(&mut self) -> Result<i32, SnapshotError> {
        Ok(self.u32()? as i32)
    }

    fn i64(&mut self) -> Result<i64, SnapshotError> {
        Ok(self.u64()? as i64)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        if self.remaining() < n {
            return Err(SnapshotError::new("unexpected end of TIRS"));
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }

    fn name(&mut self) -> Result<String, SnapshotError> {
        let len = self.u32()? as usize;
        if len > MAX_BYTES {
            return Err(SnapshotError::new("name too long"));
        }
        let bytes = self.bytes(len)?;
        let s = std::str::from_utf8(bytes).map_err(|_| SnapshotError::new("invalid UTF-8 name"))?;
        if s.contains('\0') {
            return Err(SnapshotError::new("U+0000 in name"));
        }
        Ok(s.to_string())
    }

    fn flag(&mut self) -> Result<bool, SnapshotError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(SnapshotError::new(format!("invalid flag {other}"))),
        }
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

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }

    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    fn name(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.u32(bytes.len() as u32);
        self.bytes(bytes);
    }

    fn flag(&mut self, v: bool) {
        self.u8(if v { 1 } else { 0 });
    }
}

fn write_value(w: &mut Writer, v: &Value) {
    w.u8(v.value_type().code());
    w.u8(v.label as u8);
    match v.payload {
        ValuePayload::I32(x) => w.i32(x),
        ValuePayload::I64(x) => w.i64(x),
        ValuePayload::Unit => {}
        ValuePayload::Capability(h) => {
            w.u32(h.table_index);
            w.u32(h.generation);
        }
    }
}

fn read_label(r: &mut Reader<'_>) -> Result<Label, SnapshotError> {
    let b = r.u8()?;
    if b > 3 {
        return Err(SnapshotError::new(format!("invalid sensitivity label {b}")));
    }
    Ok(match b {
        0 => Label::Public,
        1 => Label::Internal,
        2 => Label::Confidential,
        3 => Label::Secret,
        _ => unreachable!(),
    })
}

fn read_value(r: &mut Reader<'_>) -> Result<Value, SnapshotError> {
    let ty = r.u8()?;
    let label = read_label(r)?;
    let payload = match ValueType::from_code(ty) {
        Some(ValueType::I32) => ValuePayload::I32(r.i32()?),
        Some(ValueType::I64) => ValuePayload::I64(r.i64()?),
        Some(ValueType::Unit) => ValuePayload::Unit,
        Some(ValueType::Capability) => ValuePayload::Capability(CapHandle {
            table_index: r.u32()?,
            generation: r.u32()?,
        }),
        None => {
            return Err(SnapshotError::new(format!("unknown value type {ty:#04x}")));
        }
    };
    Ok(Value { payload, label })
}

fn write_control_label(w: &mut Writer, label: &ControlLabel) {
    w.u8(label.label_kind.code());
    w.u32(label.parameter_count);
    w.u32(label.result_count);
    w.u32(label.stack_height);
    w.u32(label.branch_instruction_index);
}

fn read_control_label(r: &mut Reader<'_>) -> Result<ControlLabel, SnapshotError> {
    let code = r.u8()?;
    let label_kind = crate::machine::ControlLabelKind::from_code(code)
        .ok_or_else(|| SnapshotError::new(format!("unknown labelKind {code}")))?;
    Ok(ControlLabel {
        label_kind,
        parameter_count: r.u32()?,
        result_count: r.u32()?,
        stack_height: r.u32()?,
        branch_instruction_index: r.u32()?,
    })
}

fn write_call_frame(w: &mut Writer, frame: &CallFrame) {
    w.u32(frame.function_index);
    w.u32(frame.instruction_index);
    w.u32(frame.locals.len() as u32);
    for v in &frame.locals {
        write_value(w, v);
    }
    w.u32(frame.control_stack.len() as u32);
    for label in &frame.control_stack {
        write_control_label(w, label);
    }
    match frame.return_program_counter {
        Some(pc) => {
            w.flag(true);
            w.u32(pc.function_index);
            w.u32(pc.instruction_index);
        }
        None => w.flag(false),
    }
}

fn read_call_frame(r: &mut Reader<'_>) -> Result<CallFrame, SnapshotError> {
    let function_index = r.u32()?;
    let instruction_index = r.u32()?;
    let local_count = r.u32()? as usize;
    let mut locals = Vec::with_capacity(local_count);
    for _ in 0..local_count {
        locals.push(read_value(r)?);
    }
    let label_count = r.u32()? as usize;
    let mut control_stack = Vec::with_capacity(label_count);
    for _ in 0..label_count {
        control_stack.push(read_control_label(r)?);
    }
    let return_program_counter = if r.flag()? {
        Some(ProgramCounter {
            function_index: r.u32()?,
            instruction_index: r.u32()?,
        })
    } else {
        None
    };
    Ok(CallFrame {
        function_index,
        instruction_index,
        locals,
        control_stack,
        return_program_counter,
    })
}

pub fn encode_tirs(snapshot: &CoreSnapshot) -> Vec<u8> {
    let mut w = Writer::new();
    w.bytes(MAGIC);
    w.u16(FORMAT_VERSION);
    w.u16(0);
    w.u32(snapshot.module_bytes.len() as u32);
    w.bytes(&snapshot.module_bytes);
    w.name(&snapshot.entry_name);
    w.u32(snapshot.plugin_identities.len() as u32);
    for id in &snapshot.plugin_identities {
        w.u32(id.plugin_id);
        w.bytes(&id.identity_hash);
        w.name(&id.name);
        w.name(&id.version);
    }
    w.u64(snapshot.remaining_fuel);
    let mut quotas = snapshot.quotas.clone();
    quotas.sort_by_key(|q| q.dimension);
    w.u32(quotas.len() as u32);
    for q in &quotas {
        w.u8(q.dimension.code());
        w.u64(q.remaining);
    }
    w.u32(snapshot.linear_memory.len() as u32);
    w.bytes(&snapshot.linear_memory);
    w.u32(snapshot.globals.len() as u32);
    for v in &snapshot.globals {
        write_value(&mut w, v);
    }
    w.u32(snapshot.capability_table.len() as u32);
    for e in &snapshot.capability_table {
        w.u32(e.table_index);
        w.u32(e.generation);
        w.flag(e.live);
        w.u32(e.host_identity_opaque.len() as u32);
        w.bytes(&e.host_identity_opaque);
    }
    match snapshot.active_continuation_identifier {
        Some(id) => {
            w.flag(true);
            w.u32(id);
        }
        None => w.flag(false),
    }
    w.u32(snapshot.continuations.len() as u32);
    for c in &snapshot.continuations {
        w.u32(c.continuation_identifier);
        w.u32(c.value_stack.len() as u32);
        for v in &c.value_stack {
            write_value(&mut w, v);
        }
        w.u32(c.call_frames.len() as u32);
        for frame in &c.call_frames {
            write_call_frame(&mut w, frame);
        }
    }
    w.u32(snapshot.pending_host_calls.len() as u32);
    for call in &snapshot.pending_host_calls {
        write_host_call(&mut w, call);
    }
    w.buf
}

fn write_host_call(w: &mut Writer, call: &HostCall) {
    w.u32(call.plugin_id);
    w.u32(call.method_id);
    w.u32(call.arguments.len() as u32);
    for v in &call.arguments {
        write_value(w, v);
    }
    w.u32(call.capabilities.len() as u32);
    for h in &call.capabilities {
        w.u32(h.table_index);
        w.u32(h.generation);
    }
    w.u32(call.continuation_identifier);
}

fn read_host_call(r: &mut Reader<'_>) -> Result<HostCall, SnapshotError> {
    let plugin_id = r.u32()?;
    let method_id = r.u32()?;
    let arg_count = r.u32()? as usize;
    let mut arguments = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        arguments.push(read_value(r)?);
    }
    let cap_n = r.u32()? as usize;
    let mut capabilities = Vec::with_capacity(cap_n);
    for _ in 0..cap_n {
        capabilities.push(CapHandle {
            table_index: r.u32()?,
            generation: r.u32()?,
        });
    }
    let continuation_identifier = r.u32()?;
    Ok(HostCall {
        plugin_id,
        method_id,
        arguments,
        capabilities,
        continuation_identifier,
    })
}

pub fn decode_tirs(bytes: &[u8]) -> Result<CoreSnapshot, SnapshotError> {
    if bytes.len() > MAX_BYTES {
        return Err(SnapshotError::new("TIRS too large"));
    }
    let mut r = Reader::new(bytes);
    let magic = r.bytes(4)?;
    if magic != MAGIC {
        return Err(SnapshotError::new("bad TIRS magic"));
    }
    let version = r.u16()?;
    if version != FORMAT_VERSION {
        return Err(SnapshotError::new(format!(
            "unsupported TIRS version {version}"
        )));
    }
    let reserved = r.u16()?;
    if reserved != 0 {
        return Err(SnapshotError::new("TIRS reserved must be 0"));
    }
    let module_len = r.u32()? as usize;
    let module_bytes = r.bytes(module_len)?.to_vec();
    let entry_name = r.name()?;
    let ident_count = r.u32()? as usize;
    let mut plugin_identities = Vec::with_capacity(ident_count);
    for _ in 0..ident_count {
        let plugin_id = r.u32()?;
        let hash = r.bytes(32)?;
        let mut identity_hash = [0u8; 32];
        identity_hash.copy_from_slice(hash);
        plugin_identities.push(PluginIdentity {
            plugin_id,
            identity_hash,
            name: r.name()?,
            version: r.name()?,
        });
    }
    let remaining_fuel = r.u64()?;
    let quota_count = r.u32()? as usize;
    let mut quotas = Vec::with_capacity(quota_count);
    let mut last_dim: Option<u8> = None;
    for _ in 0..quota_count {
        let code = r.u8()?;
        let Some(dimension) = QuotaDimension::from_code(code) else {
            return Err(SnapshotError::new(format!(
                "unknown quota dimension {code}"
            )));
        };
        if last_dim.is_some_and(|d| code <= d) {
            return Err(SnapshotError::new(
                "quota dimensions must be strictly increasing",
            ));
        }
        last_dim = Some(code);
        quotas.push(QuotaSlot {
            dimension,
            remaining: r.u64()?,
        });
    }
    let mem_len = r.u32()? as usize;
    let linear_memory = r.bytes(mem_len)?.to_vec();
    let global_count = r.u32()? as usize;
    let mut globals = Vec::with_capacity(global_count);
    for _ in 0..global_count {
        globals.push(read_value(&mut r)?);
    }
    let cap_count = r.u32()? as usize;
    let mut capability_table = Vec::with_capacity(cap_count);
    for _ in 0..cap_count {
        let table_index = r.u32()?;
        let generation = r.u32()?;
        let live = r.flag()?;
        let opaque_len = r.u32()? as usize;
        let host_identity_opaque = r.bytes(opaque_len)?.to_vec();
        capability_table.push(CapabilityTableEntry {
            table_index,
            generation,
            live,
            host_identity_opaque,
        });
    }
    let active_continuation_identifier = if r.flag()? { Some(r.u32()?) } else { None };
    let cont_count = r.u32()? as usize;
    if cont_count > MAX_CONTINUATIONS {
        return Err(SnapshotError::new("continuationCount exceeds cap"));
    }
    let mut continuations = Vec::with_capacity(cont_count);
    let mut seen_ids = HashSet::new();
    for _ in 0..cont_count {
        let continuation_identifier = r.u32()?;
        if !seen_ids.insert(continuation_identifier) {
            return Err(SnapshotError::new("duplicate continuationIdentifier"));
        }
        let stack_count = r.u32()? as usize;
        let mut value_stack = Vec::with_capacity(stack_count);
        for _ in 0..stack_count {
            value_stack.push(read_value(&mut r)?);
        }
        let frame_count = r.u32()? as usize;
        let mut call_frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            call_frames.push(read_call_frame(&mut r)?);
        }
        continuations.push(Continuation {
            continuation_identifier,
            value_stack,
            call_frames,
        });
    }
    if let Some(id) = active_continuation_identifier {
        if !continuations
            .iter()
            .any(|c| c.continuation_identifier == id)
        {
            return Err(SnapshotError::new(
                "activeContinuationIdentifier missing from continuations",
            ));
        }
    }
    let pending_count = r.u32()? as usize;
    if pending_count > MAX_CONTINUATIONS {
        return Err(SnapshotError::new("pendingHostCallCount exceeds cap"));
    }
    let mut pending_host_calls = Vec::with_capacity(pending_count);
    let mut pending_ids = HashSet::new();
    for _ in 0..pending_count {
        let call = read_host_call(&mut r)?;
        if !continuations
            .iter()
            .any(|c| c.continuation_identifier == call.continuation_identifier)
        {
            return Err(SnapshotError::new(
                "pending HostCall continuationIdentifier missing from continuations",
            ));
        }
        if !pending_ids.insert(call.continuation_identifier) {
            return Err(SnapshotError::new(
                "duplicate pending HostCall continuationIdentifier",
            ));
        }
        pending_host_calls.push(call);
    }
    if r.remaining() != 0 {
        return Err(SnapshotError::new("trailing TIRS bytes"));
    }
    Ok(CoreSnapshot {
        module_bytes,
        entry_name,
        plugin_identities,
        remaining_fuel,
        quotas,
        linear_memory,
        globals,
        capability_table,
        active_continuation_identifier,
        continuations,
        pending_host_calls,
    })
}

pub fn capability_handle_live(table: &[CapabilityTableEntry], handle: CapHandle) -> bool {
    if handle.is_null() {
        return true;
    }
    table
        .iter()
        .any(|e| e.table_index == handle.table_index && e.generation == handle.generation && e.live)
}

pub fn snapshot_capability_values(snapshot: &CoreSnapshot) -> Vec<Value> {
    let mut out = snapshot.globals.clone();
    for c in &snapshot.continuations {
        out.extend_from_slice(&c.value_stack);
        for frame in &c.call_frames {
            out.extend_from_slice(&frame.locals);
        }
    }
    for call in &snapshot.pending_host_calls {
        out.extend_from_slice(&call.arguments);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .filter(|t| !t.starts_with(';'))
            .flat_map(|t| {
                t.as_bytes()
                    .chunks(2)
                    .filter(|c| c.len() == 2 && c[0].is_ascii_hexdigit())
                    .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            })
            .collect()
    }

    const ECHO_SUSPEND_TIRS: &str = "
54 49 52 53 01 00 00 00
80 00 00 00
54 49 52 00 01 00 00 00
01 17 00 00 00 02 00 00 00 01 00 00 00 01 01 00 00 00 01 00 00 00 00 01 00 00 00 01
02 18 00 00 00 01 00 00 00 04 00 00 00 45 63 68 6F 00 00 00 00 00 00 00 00 00 00 00 00
03 08 00 00 00 01 00 00 00 01 00 00 00
06 11 00 00 00 01 00 00 00 04 00 00 00 6D 61 69 6E 01 00 00 00 00
07 17 00 00 00 01 00 00 00 0F 00 00 00 00 00 00 00 10 29 00 00 00 70 00 00 00 00 64
04 00 00 00 6D 61 69 6E
01 00 00 00
00 00 00 00
50 84 4C 2B CD 5A 84 FE 5B BB 6F 36 45 2B 62 CA
33 D7 78 1B 7A 65 71 9F 06 1C 0A 60 B6 72 E0 49
04 00 00 00 45 63 68 6F
05 00 00 00 31 2E 30 2E 30
E6 03 00 00 00 00 00 00
00 00 00 00
00 00 00 00
00 00 00 00
00 00 00 00
01
00 00 00 00
01 00 00 00
00 00 00 00
00 00 00 00
01 00 00 00
00 00 00 00
02 00 00 00
00 00 00 00
01 00 00 00
01
00 00 00 00
01 00 00 00
00 00 00 00
02 00 00 00
00
01 00 00 00
00 00 00 00
00 00 00 00
01 00 00 00
01 00 29 00 00 00
00 00 00 00
00 00 00 00
";

    #[test]
    fn echo_suspend_tirs_round_trip() {
        let bytes = parse_hex(ECHO_SUSPEND_TIRS);
        let snap = decode_tirs(&bytes).expect("decode TIRS");
        assert_eq!(snap.entry_name, "main");
        assert_eq!(snap.remaining_fuel, 998);
        assert!(snap.quotas.is_empty());
        assert_eq!(snap.module_bytes.len(), 128);
        assert_eq!(snap.plugin_identities.len(), 1);
        assert_eq!(snap.plugin_identities[0].name, "Echo");
        assert_eq!(snap.plugin_identities[0].version, "1.0.0");
        assert_eq!(
            snap.plugin_identities[0].identity_hash,
            crate::identity::hash_plugin_identity(&crate::identity::PluginIdentityInput {
                name: "Echo",
                version: "1.0.0",
                schema: b"(schema Echo v1)",
                metadata: b"",
                implementation_digest: None,
            })
            .unwrap()
        );
        assert_eq!(snap.active_continuation_identifier, Some(0));
        assert_eq!(snap.continuations.len(), 1);
        assert_eq!(snap.continuations[0].call_frames[0].instruction_index, 2);
        let call = snap.pending_host_calls.first().unwrap();
        assert_eq!(call.arguments, vec![Value::i32(41, Label::Public)]);
        assert_eq!(encode_tirs(&snap), bytes);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = parse_hex(ECHO_SUSPEND_TIRS);
        bytes[0] = b'X';
        assert!(decode_tirs(&bytes).is_err());
    }

    #[test]
    fn bad_version_is_rejected() {
        let mut bytes = parse_hex(ECHO_SUSPEND_TIRS);
        bytes[4] = 2;
        assert!(decode_tirs(&bytes).is_err());
    }
}
