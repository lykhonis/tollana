use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, CODE_SCHEMA_BYTES};
use std::fmt::Write;
use tollana_core::{
    decode_text, hash_canonical_bytes, CapHandle, ExecOutcome, FunctionType, Instance,
    QuotaDimension, QuotaSlot, SuspendReason, Value,
};

pub const METHOD_RUN: u32 = 0;
const PAGE_BYTES: u64 = 65536;

pub struct Code {
    allowlist: Vec<CapHandle>,
}

impl Default for Code {
    fn default() -> Self {
        Self::new()
    }
}

impl Code {
    pub fn new() -> Self {
        Self {
            allowlist: Vec::new(),
        }
    }

    pub fn with_allowlist(mut self, caps: Vec<CapHandle>) -> Self {
        self.allowlist = caps;
        self
    }

    fn granted_caps(&self, passed: &[CapHandle]) -> Vec<CapHandle> {
        passed
            .iter()
            .copied()
            .filter(|c| self.allowlist.contains(c))
            .collect()
    }

    fn run(
        &mut self,
        args: &[Value],
        caps: &[CapHandle],
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        let src_ptr = i32_arg(args, 0)?;
        let src_len = i32_arg(args, 1)?;
        let input = i32_arg(args, 2)?;
        let fuel = i32_arg(args, 3)?;
        let memory_pages = i32_arg(args, 4)?;
        let input_label = args[2].label;
        let source = ctx.read_memory(src_ptr, src_len).unwrap_or_default();
        let hash = hash_canonical_bytes(&source);
        if src_len < 0 || source.len() != src_len as usize {
            return Err(deny("invalid_source", hash));
        }
        if fuel <= 0 {
            return Err(deny("invalid_fuel", hash));
        }
        if memory_pages < 0 {
            return Err(deny("invalid_memory_pages", hash));
        }
        let text = std::str::from_utf8(&source).map_err(|_| deny("invalid_source", hash))?;
        let module = decode_text(text).map_err(|_| deny("invalid_source", hash))?;
        let max_pages = memory_pages as u32;
        let mut quotas = Vec::new();
        if max_pages > 0 {
            quotas.push(QuotaSlot {
                dimension: QuotaDimension::MemoryBytes,
                remaining: u64::from(max_pages) * PAGE_BYTES,
            });
        }
        let mut child = Instance::instantiate_with(module, &[], Vec::new(), max_pages, &quotas)
            .map_err(|e| map_instantiate(e, hash))?;
        for handle in self.granted_caps(caps) {
            child.grant_cap(handle, Vec::new());
        }
        let outcome = child
            .invoke("main", &[Value::i32(input, input_label)], fuel as u64)
            .map_err(|e| map_invoke(e, hash))?;
        match outcome {
            ExecOutcome::Completed { results } => {
                let bits = results
                    .first()
                    .and_then(|v| v.as_i32())
                    .ok_or_else(|| deny("child_result", hash))?;
                let label = results.first().map(|v| v.label).unwrap_or(input_label);
                Ok(PluginResult::Immediate(vec![Value::i32(bits, label)]))
            }
            ExecOutcome::Suspended {
                reason: SuspendReason::OutOfFuel,
            } => Err(deny("child_out_of_fuel", hash)),
            ExecOutcome::Suspended {
                reason: SuspendReason::QuotaExhausted { .. },
            } => Err(deny("child_quota", hash)),
            ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            } => Err(deny("child_host_invoke", hash)),
            ExecOutcome::Trapped { .. } => Err(deny("child_trap", hash)),
        }
    }
}

fn i32_arg(args: &[Value], index: usize) -> Result<i32, HostError> {
    args.get(index)
        .and_then(|v| v.as_i32())
        .ok_or_else(|| HostError::new("code.run expects five i32 arguments"))
}

fn hex32(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in hash {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn deny(reason: &str, hash: [u8; 32]) -> HostError {
    HostError::new(format!(
        "code_denied:{reason} source_sha256={}",
        hex32(&hash)
    ))
}

fn map_instantiate(e: tollana_core::HostInterfaceError, hash: [u8; 32]) -> HostError {
    let msg = e.to_string();
    if msg.contains("missing plugin") {
        deny("unbound_plugin", hash)
    } else if msg.contains("pageCount") || msg.contains("memory quota") {
        deny("child_memory", hash)
    } else {
        deny("child_instantiate", hash)
    }
}

fn map_invoke(e: tollana_core::HostInterfaceError, hash: [u8; 32]) -> HostError {
    match e {
        tollana_core::HostInterfaceError::HostCallPending => deny("child_host_invoke", hash),
        _ => deny("child_invoke", hash),
    }
}

impl Plugin for Code {
    fn name(&self) -> &str {
        "code"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        CODE_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(CODE_SCHEMA_BYTES)?;
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
            METHOD_RUN => self.run(args, caps, ctx),
            other => Err(HostError::new(format!("unknown code method {other}"))),
        }
    }

    fn snapshot_state(&self) -> Vec<u8> {
        Vec::new()
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if !bytes.is_empty() {
            return Err(HostError::new("code snapshot must not retain child memory"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::host::Host;
    use tollana_core::{JournalEventKind, Label};

    const CHILD_ADD: &str = r#"
(module
  (func (export "main") (param i32) (result i32)
    (i32.add (local.get 0) (i32.const 1))))
"#;

    fn parent_module(plugin_id: u32, len: i32, input: i32, fuel: i32, pages: i32) -> String {
        format!(
            r#"
(module
  (memory (pages 1))
  (host.import code.run
    (pluginId {plugin_id})
    (methodId 0)
    (param i32)
    (param i32)
    (param i32)
    (param i32)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke code.run
      (i32.const 0)
      (i32.const {len})
      (i32.const {input})
      (i32.const {fuel})
      (i32.const {pages}))))
"#
        )
    }

    fn run_child(src: &str, input: i32, fuel: i32, pages: i32) -> (Host, ExecOutcome) {
        let mut host = Host::new();
        host.register(Box::new(Code::new())).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("code").unwrap();
        let len = src.len() as i32;
        host.instantiate_text(&parent_module(id, len, input, fuel, pages))
            .unwrap();
        host.write_linear_memory(0, src.as_bytes()).unwrap();
        let out = host.run("main", &[], 1000).unwrap();
        (host, out)
    }

    #[test]
    fn child_bytecode_returns_input_plus_one() {
        let (_host, out) = run_child(CHILD_ADD, 41, 1000, 0);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(42, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn child_fuel_does_not_consume_parent_fuel() {
        let remaining = |child_fuel: i32| {
            let (host, out) = run_child(CHILD_ADD, 41, child_fuel, 0);
            match out {
                ExecOutcome::Completed { .. } => {}
                other => panic!("{other:?}"),
            }
            host.instance().unwrap().machine.remaining_fuel
        };
        assert_eq!(remaining(10), remaining(1000));
    }

    #[test]
    fn child_out_of_fuel_does_not_complete_parent() {
        let mut host = Host::new();
        host.register(Box::new(Code::new())).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("code").unwrap();
        let len = CHILD_ADD.len() as i32;
        host.instantiate_text(&parent_module(id, len, 41, 1, 0))
            .unwrap();
        host.write_linear_memory(0, CHILD_ADD.as_bytes()).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("child_out_of_fuel"), "{err}");
        assert!(
            host.instance().unwrap().machine.remaining_fuel > 990,
            "parent fuel should be independent of a starved child"
        );
    }

    #[test]
    fn default_allowlist_is_empty() {
        let granted = CapHandle {
            table_index: 1,
            generation: 1,
        };
        assert!(Code::new().granted_caps(&[granted]).is_empty());
    }

    #[test]
    fn allowlist_intersection_drops_ungranted_handles() {
        let allowed = CapHandle {
            table_index: 1,
            generation: 1,
        };
        let extra = CapHandle {
            table_index: 2,
            generation: 1,
        };
        let code = Code::new().with_allowlist(vec![allowed]);
        assert!(code.granted_caps(&[]).is_empty());
        assert_eq!(code.granted_caps(&[allowed, extra]), vec![allowed]);
    }

    const CHILD_CLOCK: &str = r#"
(module
  (host.import clock.now_wall
    (pluginId 0)
    (methodId 0)
    (result i64))
  (func (export "main") (param i32) (result i32)
    (host.invoke clock.now_wall)
    drop
    (local.get 0)))
"#;

    #[test]
    fn child_cannot_invoke_parent_clock() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(1, 0))).unwrap();
        host.register(Box::new(Code::new())).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("code").unwrap();
        let len = CHILD_CLOCK.len() as i32;
        host.instantiate_text(&parent_module(id, len, 0, 1000, 0))
            .unwrap();
        host.write_linear_memory(0, CHILD_CLOCK.as_bytes()).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("unbound_plugin"), "{err}");
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
                assert_eq!(*method_id, METHOD_RUN);
                assert!(message.contains("unbound_plugin"), "{message}");
            }
            other => panic!("{}", other.name()),
        }
    }

    const CHILD_LOAD: &str = r#"
(module
  (memory (pages 1))
  (func (export "main") (param i32) (result i32)
    (i32.load (i32.const 400))))
"#;

    const CHILD_MARK: &str = r#"
(module
  (memory (pages 1))
  (func (export "main") (param i32) (result i32)
    (i32.store (i32.const 0) (i32.const 0x01020304))
    (i32.add (local.get 0) (i32.const 1))))
"#;

    const MARK: [u8; 4] = [0x04, 0x03, 0x02, 0x01];

    #[test]
    fn child_cannot_observe_parent_memory() {
        let mut host = Host::new();
        host.register(Box::new(Code::new())).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("code").unwrap();
        let src = CHILD_LOAD.as_bytes();
        host.instantiate_text(&parent_module(id, src.len() as i32, 0, 1000, 1))
            .unwrap();
        host.write_linear_memory(0, src).unwrap();
        host.write_linear_memory(400, &[0x44, 0x33, 0x22, 0x11])
            .unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(0, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            host.read_linear_memory(400, 4).unwrap(),
            [0x44, 0x33, 0x22, 0x11]
        );
    }

    #[test]
    fn parent_snapshot_does_not_inline_child_heap() {
        let (mut host, out) = run_child(CHILD_MARK, 41, 1000, 1);
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(42, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert_ne!(&host.instance().unwrap().machine.linear_memory[0..4], &MARK);
        let bytes = host.snapshot().unwrap();
        let decoded = tollana_core::decode_container(&bytes, None).unwrap();
        let core = tollana_core::decode_tirs(&decoded.body.tirs).unwrap();
        assert!(
            !contains_mark(&core.linear_memory),
            "parent TIRS memory must not contain child heap"
        );
        for entry in &decoded.body.plugin_state {
            assert!(
                !contains_mark(&entry.blob),
                "code plugin blob must not retain child heap"
            );
        }
    }

    fn contains_mark(bytes: &[u8]) -> bool {
        bytes.windows(4).any(|w| w == MARK)
    }

    #[test]
    fn denial_journals_source_sha256() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(1, 0))).unwrap();
        host.register(Box::new(Code::new())).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("code").unwrap();
        let src = CHILD_CLOCK.as_bytes();
        host.instantiate_text(&parent_module(id, src.len() as i32, 0, 1000, 0))
            .unwrap();
        host.write_linear_memory(0, src).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        let expected = format!("source_sha256={}", hex32(&hash_canonical_bytes(src)));
        assert!(err.message.contains(&expected), "{err}");
        let failed = host
            .instance()
            .unwrap()
            .journal
            .events
            .iter()
            .find(|e| e.kind.name() == "HostCallFailed")
            .unwrap();
        match &failed.kind {
            JournalEventKind::HostCallFailed { message, .. } => {
                assert!(message.contains(&expected), "{message}");
            }
            other => panic!("{}", other.name()),
        }
    }
}
