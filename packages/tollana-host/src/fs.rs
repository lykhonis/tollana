use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, FS_SCHEMA_BYTES};
use std::collections::{BTreeMap, HashMap};
use tollana_core::{CapHandle, FunctionType, Label, QuotaDimension, Value};

pub const METHOD_READ: u32 = 0;
pub const METHOD_WRITE: u32 = 1;
pub const METHOD_LIST: u32 = 2;

#[derive(Clone, Debug)]
pub struct Fs {
    files: BTreeMap<String, i32>,
    preopens: HashMap<CapHandle, String>,
}

impl Default for Fs {
    fn default() -> Self {
        Self::memory()
    }
}

impl Fs {
    pub fn memory() -> Self {
        Self {
            files: BTreeMap::new(),
            preopens: HashMap::new(),
        }
    }

    pub fn preopen(&mut self, handle: CapHandle, root: &str) {
        self.preopens.insert(handle, normalize_root(root));
    }

    pub fn insert_file(&mut self, path: &str, payload: i32) {
        self.files.insert(normalize_root(path), payload);
    }

    fn resolve(&self, cap: CapHandle, rel: &str) -> Result<String, HostError> {
        let root = self
            .preopens
            .get(&cap)
            .ok_or_else(|| HostError::new("unknown_preopen"))?;
        confined_join(root, rel)
    }

    fn guest_path(
        &self,
        handle: CapHandle,
        caps: &[CapHandle],
        args: &[Value],
        ctx: &dyn PluginContext,
    ) -> Result<String, HostError> {
        if !caps.contains(&handle) {
            return Err(HostError::new("unknown_preopen"));
        }
        let ptr = i32_arg(args, 1)?;
        let len = i32_arg(args, 2)?;
        let bytes = ctx.read_memory(ptr, len)?;
        let rel =
            std::str::from_utf8(&bytes).map_err(|_| HostError::new("invalid fs path utf8"))?;
        self.resolve(handle, rel)
    }

    fn charge_io(ctx: &mut dyn PluginContext, amount: u64) -> Result<(), HostError> {
        if ctx.quota_remaining(QuotaDimension::IoBytes).is_some()
            && !ctx.consume_quota(QuotaDimension::IoBytes, amount)
        {
            return Err(HostError::new("io_bytes_quota"));
        }
        Ok(())
    }
}

fn normalize_root(path: &str) -> String {
    let parts = virtual_parts(path);
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn virtual_parts(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect()
}

fn confined_join(root: &str, rel: &str) -> Result<String, HostError> {
    let root_parts: Vec<String> = virtual_parts(root)
        .into_iter()
        .map(ToString::to_string)
        .collect();
    let abs = rel.starts_with('/');
    let mut parts = if abs { Vec::new() } else { root_parts.clone() };
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if abs {
                    if parts.is_empty() {
                        return Err(HostError::new("fs_escape"));
                    }
                    parts.pop();
                } else if parts.len() <= root_parts.len() {
                    return Err(HostError::new("fs_escape"));
                } else {
                    parts.pop();
                }
            }
            p => parts.push(p.to_string()),
        }
    }
    if abs && (parts.len() < root_parts.len() || parts[..root_parts.len()] != root_parts[..]) {
        return Err(HostError::new("fs_escape"));
    }
    if parts.is_empty() {
        Ok("/".into())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn under_root(path: &str, root: &str) -> bool {
    path == root || (root == "/") || path.starts_with(&format!("{root}/"))
}

fn cap_arg(args: &[Value], index: usize) -> Result<CapHandle, HostError> {
    args.get(index)
        .and_then(|v| v.as_cap())
        .filter(|h| !h.is_null())
        .ok_or_else(|| HostError::new("fs methods expect a live capability"))
}

fn i32_arg(args: &[Value], index: usize) -> Result<i32, HostError> {
    args.get(index)
        .and_then(|v| v.as_i32())
        .ok_or_else(|| HostError::new("fs methods expect i32"))
}

fn push_u32(buf: &mut Vec<u8>, n: u32) {
    buf.extend_from_slice(&n.to_le_bytes());
}

fn push_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    push_u32(buf, bytes.len() as u32);
    buf.extend_from_slice(bytes);
}

impl Plugin for Fs {
    fn name(&self) -> &str {
        "fs"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        FS_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(FS_SCHEMA_BYTES)?;
        schema
            .methods
            .iter()
            .map(|m| Ok((m.id, function_type(m)?)))
            .collect()
    }

    fn invoke(
        &mut self,
        method_id: u32,
        args: &[Value],
        caps: &[CapHandle],
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        match method_id {
            METHOD_READ => {
                let handle = cap_arg(args, 0)?;
                let path = self.guest_path(handle, caps, args, ctx)?;
                Self::charge_io(ctx, 4)?;
                let payload = self
                    .files
                    .get(&path)
                    .copied()
                    .ok_or_else(|| HostError::new("not_found"))?;
                Ok(PluginResult::Immediate(vec![Value::i32(
                    payload,
                    Label::Public,
                )]))
            }
            METHOD_WRITE => {
                let handle = cap_arg(args, 0)?;
                let path = self.guest_path(handle, caps, args, ctx)?;
                let payload = i32_arg(args, 3)?;
                Self::charge_io(ctx, 4)?;
                self.files.insert(path, payload);
                Ok(PluginResult::Immediate(vec![Value::unit(Label::Public)]))
            }
            METHOD_LIST => {
                let handle = cap_arg(args, 0)?;
                if args.len() != 1 {
                    return Err(HostError::new("fs.list expects a capability"));
                }
                if !caps.contains(&handle) {
                    return Err(HostError::new("unknown_preopen"));
                }
                let root = self
                    .preopens
                    .get(&handle)
                    .ok_or_else(|| HostError::new("unknown_preopen"))?;
                let count = self.files.keys().filter(|p| under_root(p, root)).count() as i32;
                Ok(PluginResult::Immediate(vec![Value::i32(
                    count,
                    Label::Public,
                )]))
            }
            other => Err(HostError::new(format!("unknown fs method {other}"))),
        }
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = vec![1];
        push_u32(&mut buf, self.files.len() as u32);
        for (path, payload) in &self.files {
            push_str(&mut buf, path);
            buf.extend_from_slice(&payload.to_le_bytes());
        }
        let mut preopens: Vec<(CapHandle, &String)> =
            self.preopens.iter().map(|(k, v)| (*k, v)).collect();
        preopens.sort_by_key(|(h, _)| (h.table_index, h.generation));
        push_u32(&mut buf, preopens.len() as u32);
        for (handle, root) in preopens {
            buf.extend_from_slice(&handle.table_index.to_le_bytes());
            buf.extend_from_slice(&handle.generation.to_le_bytes());
            push_str(&mut buf, root);
        }
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.is_empty() || bytes[0] != 1 {
            return Err(HostError::new("invalid fs snapshot blob"));
        }
        let mut pos = 1;
        let n_files = read_u32(bytes, &mut pos)?;
        self.files.clear();
        for _ in 0..n_files {
            let path = read_str(bytes, &mut pos)?;
            let payload = read_i32(bytes, &mut pos)?;
            self.files.insert(path, payload);
        }
        let n_pre = read_u32(bytes, &mut pos)?;
        self.preopens.clear();
        for _ in 0..n_pre {
            let table_index = read_u32(bytes, &mut pos)?;
            let generation = read_u32(bytes, &mut pos)?;
            let root = read_str(bytes, &mut pos)?;
            self.preopens.insert(
                CapHandle {
                    table_index,
                    generation,
                },
                root,
            );
        }
        if pos != bytes.len() {
            return Err(HostError::new("trailing fs snapshot bytes"));
        }
        Ok(())
    }
}

fn read_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, HostError> {
    if *pos + 4 > bytes.len() {
        return Err(HostError::new("truncated fs snapshot blob"));
    }
    let v = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

fn read_i32(bytes: &[u8], pos: &mut usize) -> Result<i32, HostError> {
    if *pos + 4 > bytes.len() {
        return Err(HostError::new("truncated fs snapshot blob"));
    }
    let v = i32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

fn read_str(bytes: &[u8], pos: &mut usize) -> Result<String, HostError> {
    let len = read_u32(bytes, pos)? as usize;
    if *pos + len > bytes.len() {
        return Err(HostError::new("truncated fs snapshot blob"));
    }
    let s = String::from_utf8(bytes[*pos..*pos + len].to_vec())
        .map_err(|_| HostError::new("invalid fs path utf8"))?;
    *pos += len;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use tollana_core::{ExecOutcome, JournalEventKind, Label};

    const PREOPEN: CapHandle = CapHandle {
        table_index: 1,
        generation: 1,
    };

    fn path_module(plugin_id: u32, method_id: u32, extra_params: &str, body: &str) -> String {
        format!(
            r#"
(module
  (memory (pages 1))
  (host.import fs.op
    (pluginId {plugin_id})
    (methodId {method_id})
    (param Capability)
    (param i32)
    (param i32)
    {extra_params})
  (func (export "main") (param Capability) (result i32)
    {body}))
"#
        )
    }

    fn read_module(plugin_id: u32, ptr: i32, len: i32) -> String {
        path_module(
            plugin_id,
            0,
            "(result i32)",
            &format!("(host.invoke fs.op (local.get 0) (i32.const {ptr}) (i32.const {len}))"),
        )
    }

    fn write_read_module(plugin_id: u32, ptr: i32, len: i32) -> String {
        format!(
            r#"
(module
  (memory (pages 1))
  (host.import fs.write
    (pluginId {plugin_id})
    (methodId 1)
    (param Capability)
    (param i32)
    (param i32)
    (param i32)
    (result unit))
  (host.import fs.read
    (pluginId {plugin_id})
    (methodId 0)
    (param Capability)
    (param i32)
    (param i32)
    (result i32))
  (host.import fs.list
    (pluginId {plugin_id})
    (methodId 2)
    (param Capability)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke fs.write (local.get 0) (i32.const {ptr}) (i32.const {len}) (i32.const 42))
    drop
    (i32.add
      (host.invoke fs.list (local.get 0))
      (host.invoke fs.read (local.get 0) (i32.const {ptr}) (i32.const {len})))))
"#
        )
    }

    fn cap() -> Value {
        Value::capability(PREOPEN, Label::Public)
    }

    fn load_path(host: &mut Host, path: &str) -> (i32, i32) {
        host.write_linear_memory(0, path.as_bytes()).unwrap();
        (0, path.len() as i32)
    }

    #[test]
    fn write_read_list_under_preopen() {
        let mut fs = Fs::memory();
        fs.preopen(PREOPEN, "/workspace");
        let mut host = Host::new();
        host.register(Box::new(fs)).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("fs").unwrap();
        let path = "note.txt";
        host.instantiate_text(&write_read_module(id, 0, path.len() as i32))
            .unwrap();
        host.grant_cap(PREOPEN, b"fs:/workspace".to_vec()).unwrap();
        load_path(&mut host, path);
        match host.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(43, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn preopen_cannot_escape_capability_root() {
        let mut fs = Fs::memory();
        fs.preopen(PREOPEN, "/workspace");
        fs.insert_file("/secret", 99);
        let mut host = Host::new();
        host.register(Box::new(fs)).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("fs").unwrap();
        let path = "../secret";
        host.instantiate_text(&read_module(id, 0, path.len() as i32))
            .unwrap();
        host.grant_cap(PREOPEN, b"fs:/workspace".to_vec()).unwrap();
        load_path(&mut host, path);
        let err = host.run("main", &[cap()], 1000).unwrap_err();
        assert!(err.message.contains("fs_escape"), "{err}");
        let failed = host
            .instance()
            .unwrap()
            .journal
            .events
            .iter()
            .find(|e| e.kind.name() == "HostCallFailed")
            .unwrap();
        match &failed.kind {
            JournalEventKind::HostCallFailed {
                plugin_id,
                method_id,
                message,
                ..
            } => {
                assert_eq!(*plugin_id, id);
                assert_eq!(*method_id, METHOD_READ);
                assert!(message.contains("fs_escape"));
            }
            other => panic!("{}", other.name()),
        }
    }

    #[test]
    fn snapshot_restores_files_and_preopens() {
        let mut fs = Fs::memory();
        fs.preopen(PREOPEN, "/workspace");
        fs.insert_file("/workspace/note.txt", 7);
        let mut host = Host::new();
        host.register(Box::new(fs)).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("fs").unwrap();
        let path = "note.txt";
        host.instantiate_text(&read_module(id, 0, path.len() as i32))
            .unwrap();
        host.grant_cap(PREOPEN, b"fs:/workspace".to_vec()).unwrap();
        load_path(&mut host, path);
        match host.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(7, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let bytes = host.snapshot().unwrap();
        let mut host2 = Host::new();
        host2.register(Box::new(Fs::memory())).unwrap();
        host2.restore(&bytes).unwrap();
        match host2.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(7, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }
}
