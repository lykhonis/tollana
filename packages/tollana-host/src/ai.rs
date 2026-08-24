use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, AI_SCHEMA_BYTES};
use std::collections::BTreeMap;
use tollana_core::{CapHandle, FunctionType, Label, QuotaDimension, Value};

pub const METHOD_CHAT: u32 = 0;
pub const METHOD_GENERATE: u32 = 1;
pub const METHOD_EMBED: u32 = 2;

#[derive(Clone, Debug)]
pub struct Ai {
    model: String,
    external: bool,
    pending: bool,
    latency_millis: u64,
    replies: BTreeMap<i32, i32>,
    samples: Vec<(u32, i64)>,
}

impl Ai {
    pub fn local() -> Self {
        Self {
            model: "local-stub".into(),
            external: false,
            pending: false,
            latency_millis: 5,
            replies: BTreeMap::new(),
            samples: Vec::new(),
        }
    }

    pub fn external() -> Self {
        Self {
            model: "external-stub".into(),
            external: true,
            pending: false,
            latency_millis: 25,
            replies: BTreeMap::new(),
            samples: Vec::new(),
        }
    }

    pub fn reply(mut self, prompt: i32, completion: i32) -> Self {
        self.replies.insert(prompt, completion);
        self
    }

    pub fn pending(mut self) -> Self {
        self.pending = true;
        self
    }

    pub fn is_external(&self) -> bool {
        self.external
    }

    fn complete(&self, prompt: i32) -> i32 {
        self.replies.get(&prompt).copied().unwrap_or(prompt)
    }

    fn call(
        &mut self,
        method_id: u32,
        args: &[Value],
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        let prompt = args
            .first()
            .ok_or_else(|| HostError::new("ai methods expect one i32"))?;
        let Some(bits) = prompt.as_i32() else {
            return Err(HostError::new("ai methods expect one i32"));
        };
        if self.external && prompt.label >= Label::Confidential {
            return Err(HostError::new("external_confidential"));
        }
        let tokens_in = 1;
        let tokens_out = 1;
        if ctx.quota_remaining(QuotaDimension::Tokens).is_some()
            && !ctx.consume_quota(QuotaDimension::Tokens, tokens_in + tokens_out)
        {
            return Err(HostError::new("tokens_quota"));
        }
        let reply = self.complete(bits);
        self.samples.push((method_id, reply as i64));
        if self.pending {
            return Ok(PluginResult::Pending(0));
        }
        Ok(PluginResult::Immediate(vec![Value::i32(
            reply,
            prompt.label,
        )]))
    }
}

impl Plugin for Ai {
    fn name(&self) -> &str {
        "ai"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        AI_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(AI_SCHEMA_BYTES)?;
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
            METHOD_CHAT | METHOD_GENERATE | METHOD_EMBED => self.call(method_id, args, ctx),
            other => Err(HostError::new(format!("unknown ai method {other}"))),
        }
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(1);
        buf.push(u8::from(self.external));
        buf.push(u8::from(self.pending));
        buf.extend_from_slice(&self.latency_millis.to_le_bytes());
        let model = self.model.as_bytes();
        buf.extend_from_slice(&(model.len() as u32).to_le_bytes());
        buf.extend_from_slice(model);
        buf.extend_from_slice(&(self.replies.len() as u32).to_le_bytes());
        for (k, v) in &self.replies {
            buf.extend_from_slice(&k.to_le_bytes());
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.len() < 14 || bytes[0] != 1 {
            return Err(HostError::new("invalid ai snapshot blob"));
        }
        self.external = bytes[1] != 0;
        self.pending = bytes[2] != 0;
        self.latency_millis = u64::from_le_bytes(bytes[3..11].try_into().unwrap());
        let mut pos = 11;
        let name_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + name_len > bytes.len() {
            return Err(HostError::new("truncated ai snapshot blob"));
        }
        self.model = String::from_utf8(bytes[pos..pos + name_len].to_vec())
            .map_err(|_| HostError::new("invalid ai model utf8"))?;
        pos += name_len;
        if pos + 4 > bytes.len() {
            return Err(HostError::new("truncated ai snapshot blob"));
        }
        let n = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        self.replies.clear();
        for _ in 0..n {
            if pos + 8 > bytes.len() {
                return Err(HostError::new("truncated ai snapshot blob"));
            }
            let k = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let v = i32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            pos += 4;
            self.replies.insert(k, v);
        }
        if pos != bytes.len() {
            return Err(HostError::new("trailing ai snapshot bytes"));
        }
        Ok(())
    }

    fn recorded_samples(&self) -> Vec<(u32, i64)> {
        self.samples.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::host::Host;
    use tollana_core::{ExecOutcome, Label, QuotaSlot, SuspendReason};

    fn chat_module(ai_id: u32) -> String {
        format!(
            r#"
(module
  (host.import ai.chat
    (pluginId {ai_id})
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke ai.chat (i32.const 41))))
"#
        )
    }

    fn chat_read_module(ai_id: u32, ctx_id: u32) -> String {
        format!(
            r#"
(module
  (host.import ai.chat
    (pluginId {ai_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import context.read
    (pluginId {ctx_id})
    (methodId 1)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke ai.chat (host.invoke context.read (i32.const 1)))))
"#
        )
    }

    #[test]
    fn guest_chats_via_assigned_id() {
        let mut host = Host::new();
        host.register(Box::new(Ai::local().reply(41, 42))).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("ai").unwrap();
        host.instantiate_text(&chat_module(id)).unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(42, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(host.plugin_samples("ai"), [(METHOD_CHAT, 42)]);
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallResumed"));
    }

    #[test]
    fn same_module_two_backends() {
        let mut local = Host::new();
        local
            .register(Box::new(Ai::local().reply(41, 100)))
            .unwrap();
        local.bind().unwrap();
        let id = local.plugin_id("ai").unwrap();
        let src = chat_module(id);
        local.instantiate_text(&src).unwrap();
        match local.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(100, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }

        let mut other = Host::new();
        other
            .register(Box::new(Ai::local().reply(41, 200)))
            .unwrap();
        other.bind().unwrap();
        assert_eq!(other.plugin_id("ai"), Some(id));
        other.instantiate_text(&src).unwrap();
        match other.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(200, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mid_await_snapshot_restore_resume() {
        let mut host = Host::new();
        host.register(Box::new(Ai::local().reply(41, 42).pending()))
            .unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("ai").unwrap();
        host.instantiate_text(&chat_module(id)).unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            } => {}
            other => panic!("{other:?}"),
        }
        let call = host.instance().unwrap().machine.pending_host_calls[0].clone();
        assert_eq!(call.arguments, vec![Value::i32(41, Label::Public)]);
        let bytes = host.snapshot().unwrap();

        let mut host2 = Host::new();
        host2
            .register(Box::new(Ai::local().reply(41, 42).pending()))
            .unwrap();
        host2.restore(&bytes).unwrap();
        match host2
            .resume_continuation(
                call.continuation_identifier,
                vec![Value::i32(42, Label::Public)],
            )
            .unwrap()
        {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(42, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn external_denies_confidential() {
        let mut host = Host::new();
        host.register(Box::new(Ai::external().reply(7, 1))).unwrap();
        let mut ctx = Context::new();
        ctx.insert("docs://secret", 7, Label::Confidential);
        host.register(Box::new(ctx)).unwrap();
        host.bind().unwrap();
        let ai_id = host.plugin_id("ai").unwrap();
        let ctx_id = host.plugin_id("context").unwrap();
        host.instantiate_text(&chat_read_module(ai_id, ctx_id))
            .unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("external_confidential"), "{err}");
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallFailed"));
        let failed = host
            .instance()
            .unwrap()
            .journal
            .events
            .iter()
            .find(|e| e.kind.name() == "HostCallFailed")
            .unwrap();
        match &failed.kind {
            tollana_core::JournalEventKind::HostCallFailed {
                plugin_id,
                method_id,
                message,
                ..
            } => {
                assert_eq!(*plugin_id, ai_id);
                assert_eq!(*method_id, METHOD_CHAT);
                assert!(message.contains("external_confidential"));
            }
            other => panic!("{}", other.name()),
        }
    }

    #[test]
    fn local_allows_confidential_from_context() {
        let mut host = Host::new();
        host.register(Box::new(Ai::local().reply(7, 8))).unwrap();
        let mut ctx = Context::new();
        ctx.insert("docs://secret", 7, Label::Confidential);
        host.register(Box::new(ctx)).unwrap();
        host.bind().unwrap();
        let ai_id = host.plugin_id("ai").unwrap();
        let ctx_id = host.plugin_id("context").unwrap();
        host.instantiate_text(&chat_read_module(ai_id, ctx_id))
            .unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(8, Label::Confidential)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tokens_quota_denies() {
        let mut host = Host::new();
        host.register(Box::new(Ai::local().reply(41, 42))).unwrap();
        host.set_quotas(vec![QuotaSlot {
            dimension: QuotaDimension::Tokens,
            remaining: 1,
        }]);
        host.bind().unwrap();
        let id = host.plugin_id("ai").unwrap();
        host.instantiate_text(&chat_module(id)).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("tokens_quota"), "{err}");
    }
}
