use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, CONTEXT_SCHEMA_BYTES};
use std::collections::BTreeMap;
use tollana_core::{CapHandle, FunctionType, JournalEventKind, Label, Value};

pub const METHOD_LIST: u32 = 0;
pub const METHOD_READ: u32 = 1;

#[derive(Clone, Debug)]
struct Resource {
    uri: String,
    payload: i32,
    label: Label,
}

#[derive(Clone, Debug)]
pub struct Context {
    next_id: u32,
    resources: BTreeMap<u32, Resource>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            resources: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, uri: &str, payload: i32, label: Label) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.resources.insert(
            id,
            Resource {
                uri: uri.to_string(),
                payload,
                label,
            },
        );
        id
    }

    pub fn get(&self, id: u32) -> Option<(&str, i32, Label)> {
        self.resources
            .get(&id)
            .map(|r| (r.uri.as_str(), r.payload, r.label))
    }
}

impl Plugin for Context {
    fn name(&self) -> &str {
        "context"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        CONTEXT_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(CONTEXT_SCHEMA_BYTES)?;
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
        _caps: &[CapHandle],
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        match method_id {
            METHOD_LIST => {
                if !args.is_empty() {
                    return Err(HostError::new("context.list takes no arguments"));
                }
                let count = self.resources.len() as u32;
                ctx.emit(JournalEventKind::ContextListed { count });
                Ok(PluginResult::Immediate(vec![Value::i32(
                    count as i32,
                    Label::Public,
                )]))
            }
            METHOD_READ => {
                let id = args
                    .first()
                    .and_then(|v| v.as_i32())
                    .ok_or_else(|| HostError::new("context.read expects i32 id"))?;
                if id <= 0 {
                    return Err(HostError::new("unknown context resource"));
                }
                let resource = self
                    .resources
                    .get(&(id as u32))
                    .ok_or_else(|| HostError::new("unknown context resource"))?;
                ctx.emit(JournalEventKind::ContextRead {
                    resource_id: id as u32,
                    label: resource.label,
                });
                Ok(PluginResult::Immediate(vec![Value::i32(
                    resource.payload,
                    resource.label,
                )]))
            }
            other => Err(HostError::new(format!("unknown context method {other}"))),
        }
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(1);
        buf.extend_from_slice(&self.next_id.to_le_bytes());
        buf.extend_from_slice(&(self.resources.len() as u32).to_le_bytes());
        for (id, r) in &self.resources {
            buf.extend_from_slice(&id.to_le_bytes());
            let uri = r.uri.as_bytes();
            buf.extend_from_slice(&(uri.len() as u32).to_le_bytes());
            buf.extend_from_slice(uri);
            buf.extend_from_slice(&r.payload.to_le_bytes());
            buf.push(r.label as u8);
        }
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.len() < 9 || bytes[0] != 1 {
            return Err(HostError::new("invalid context snapshot blob"));
        }
        let mut pos = 1;
        self.next_id = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let n = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        self.resources.clear();
        for _ in 0..n {
            if pos + 8 > bytes.len() {
                return Err(HostError::new("truncated context snapshot blob"));
            }
            let id = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let uri_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + uri_len + 5 > bytes.len() {
                return Err(HostError::new("truncated context snapshot blob"));
            }
            let uri = String::from_utf8(bytes[pos..pos + uri_len].to_vec())
                .map_err(|_| HostError::new("invalid context uri utf8"))?;
            pos += uri_len;
            let payload = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let label = match bytes[pos] {
                0 => Label::Public,
                1 => Label::Internal,
                2 => Label::Confidential,
                3 => Label::Secret,
                _ => return Err(HostError::new("invalid context label")),
            };
            pos += 1;
            self.resources.insert(
                id,
                Resource {
                    uri,
                    payload,
                    label,
                },
            );
        }
        if pos != bytes.len() {
            return Err(HostError::new("trailing context snapshot bytes"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use tollana_core::{ExecOutcome, Label};

    fn read_module(ctx_id: u32) -> String {
        format!(
            r#"
(module
  (host.import context.list
    (pluginId {ctx_id})
    (methodId 0)
    (result i32))
  (host.import context.read
    (pluginId {ctx_id})
    (methodId 1)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke context.list)
      (host.invoke context.read (i32.const 1)))))
"#
        )
    }

    #[test]
    fn list_and_read_carry_labels() {
        let mut host = Host::new();
        let mut ctx = Context::new();
        let id = ctx.insert("docs://a", 10, Label::Internal);
        assert_eq!(id, 1);
        host.register(Box::new(ctx)).unwrap();
        host.bind().unwrap();
        let ctx_id = host.plugin_id("context").unwrap();
        host.instantiate_text(&read_module(ctx_id)).unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(11, Label::Internal)]);
            }
            other => panic!("{other:?}"),
        }
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"ContextListed"));
        assert!(names.contains(&"ContextRead"));
    }

    #[test]
    fn snapshot_restores_resources() {
        let mut host = Host::new();
        let mut ctx = Context::new();
        ctx.insert("docs://a", 10, Label::Confidential);
        host.register(Box::new(ctx)).unwrap();
        host.bind().unwrap();
        let ctx_id = host.plugin_id("context").unwrap();
        host.instantiate_text(&format!(
            r#"
(module
  (host.import context.read
    (pluginId {ctx_id})
    (methodId 1)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke context.read (i32.const 1))))
"#
        ))
        .unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(10, Label::Confidential)]);
            }
            other => panic!("{other:?}"),
        }
        let bytes = host.snapshot().unwrap();
        let mut host2 = Host::new();
        host2.register(Box::new(Context::new())).unwrap();
        host2.restore(&bytes).unwrap();
        match host2.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(10, Label::Confidential)]);
            }
            other => panic!("{other:?}"),
        }
    }
}
