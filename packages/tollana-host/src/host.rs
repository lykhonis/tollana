use crate::error::HostError;
use crate::plugin::{Plugin, PluginResult};
use std::collections::HashMap;
use tollana_core::{
    assign_local_ids, decode_text, hash_plugin_identity, ExecOutcome, HostRebind, Instance, Module,
    PluginBinding, PluginIdentity, PluginIdentityInput, PluginStateEntry, SuspendReason, Value,
};

struct Slot {
    plugin: Box<dyn Plugin>,
    hash: [u8; 32],
    plugin_id: u32,
}

pub struct Host {
    slots: Vec<Slot>,
    bound: bool,
    instance: Option<Instance>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            bound: false,
            instance: None,
        }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<(), HostError> {
        if self.instance.is_some() {
            return Err(HostError::new("cannot register after instantiate"));
        }
        let hash = hash_plugin(&*plugin)?;
        self.slots.push(Slot {
            plugin,
            hash,
            plugin_id: 0,
        });
        self.bound = false;
        Ok(())
    }

    pub fn bind(&mut self) -> Result<(), HostError> {
        let hashes: Vec<[u8; 32]> = self.slots.iter().map(|s| s.hash).collect();
        let ids = assign_local_ids(&hashes)?;
        for (slot, id) in self.slots.iter_mut().zip(ids) {
            slot.plugin_id = id;
        }
        self.bound = true;
        Ok(())
    }

    pub fn plugin_id(&self, name: &str) -> Option<u32> {
        if !self.bound {
            return None;
        }
        self.slots
            .iter()
            .find(|s| s.plugin.name() == name)
            .map(|s| s.plugin_id)
    }

    pub fn plugin_samples(&self, name: &str) -> Vec<(u32, i64)> {
        self.slots
            .iter()
            .find(|s| s.plugin.name() == name)
            .map(|s| s.plugin.recorded_samples())
            .unwrap_or_default()
    }

    pub fn instantiate_text(&mut self, src: &str) -> Result<(), HostError> {
        let module = decode_text(src).map_err(|e| HostError::new(e.message))?;
        self.instantiate(module)
    }

    pub fn instantiate(&mut self, module: Module) -> Result<(), HostError> {
        if !self.bound {
            self.bind()?;
        }
        let mut bindings = Vec::new();
        for slot in &self.slots {
            bindings.push(PluginBinding {
                identity: PluginIdentity {
                    plugin_id: slot.plugin_id,
                    identity_hash: slot.hash,
                    name: slot.plugin.name().to_string(),
                    version: slot.plugin.version().to_string(),
                },
                methods: slot.plugin.methods()?,
            });
        }
        self.instance = Some(Instance::instantiate_with(
            module,
            &bindings,
            Vec::new(),
            16,
        )?);
        Ok(())
    }

    pub fn instance(&self) -> Option<&Instance> {
        self.instance.as_ref()
    }

    pub fn instance_mut(&mut self) -> Option<&mut Instance> {
        self.instance.as_mut()
    }

    pub fn run(
        &mut self,
        export: &str,
        args: &[Value],
        fuel: u64,
    ) -> Result<ExecOutcome, HostError> {
        let outcome = self
            .instance
            .as_mut()
            .ok_or_else(|| HostError::new("not instantiated"))?
            .invoke(export, args, fuel)?;
        self.drive(outcome)
    }

    pub fn continue_run(&mut self) -> Result<ExecOutcome, HostError> {
        let outcome = self
            .instance
            .as_mut()
            .ok_or_else(|| HostError::new("not instantiated"))?
            .continue_run()?;
        self.drive(outcome)
    }

    fn drive(&mut self, mut outcome: ExecOutcome) -> Result<ExecOutcome, HostError> {
        loop {
            match &outcome {
                ExecOutcome::Suspended {
                    reason: SuspendReason::HostInvoke,
                } => {
                    outcome = self.dispatch()?;
                    if matches!(
                        outcome,
                        ExecOutcome::Suspended {
                            reason: SuspendReason::HostInvoke
                        }
                    ) && self
                        .instance
                        .as_ref()
                        .and_then(|i| i.machine.pending_host_call.as_ref())
                        .is_some()
                    {
                        return Ok(outcome);
                    }
                }
                _ => return Ok(outcome),
            }
        }
    }

    fn dispatch(&mut self) -> Result<ExecOutcome, HostError> {
        let call = self
            .instance
            .as_ref()
            .and_then(|i| i.machine.pending_host_call.clone())
            .ok_or_else(|| HostError::new("no pending host call"))?;
        let idx = self
            .slots
            .iter()
            .position(|s| s.plugin_id == call.plugin_id)
            .ok_or_else(|| HostError::new(format!("unbound plugin {}", call.plugin_id)))?;
        let result =
            self.slots[idx]
                .plugin
                .invoke(call.method_id, &call.arguments, &call.capabilities)?;
        match result {
            PluginResult::Immediate(values) => {
                let inst = self.instance.as_mut().unwrap();
                Ok(inst.resume(values)?)
            }
            PluginResult::Pending(_) => Ok(ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            }),
        }
    }

    pub fn snapshot(&mut self) -> Result<Vec<u8>, HostError> {
        let plugin_state: Vec<PluginStateEntry> = self
            .slots
            .iter()
            .map(|s| PluginStateEntry {
                plugin_id: s.plugin_id,
                blob: s.plugin.snapshot_state(),
            })
            .collect();
        let inst = self
            .instance
            .as_mut()
            .ok_or_else(|| HostError::new("not instantiated"))?;
        Ok(inst.snapshot(plugin_state))
    }

    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if !self.bound {
            self.bind()?;
        }
        let rebind = self.rebind_for_bytes(bytes)?;
        let restored = Instance::restore(bytes, &rebind, None, None)?;
        let identities: HashMap<u32, [u8; 32]> = restored
            .instance
            .plugin_identities()
            .iter()
            .map(|id| (id.plugin_id, id.identity_hash))
            .collect();
        for ident in restored.instance.plugin_identities() {
            let slot = self
                .slots
                .iter_mut()
                .find(|s| s.hash == ident.identity_hash)
                .ok_or_else(|| HostError::new(format!("no plugin for hash of {}", ident.name)))?;
            slot.plugin_id = ident.plugin_id;
        }
        for entry in &restored.plugin_state {
            let hash = identities.get(&entry.plugin_id).ok_or_else(|| {
                HostError::new(format!("blob for unknown plugin {}", entry.plugin_id))
            })?;
            let slot = self
                .slots
                .iter_mut()
                .find(|s| s.hash == *hash)
                .ok_or_else(|| HostError::new("plugin state hash mismatch"))?;
            slot.plugin.restore_state(&entry.blob)?;
        }
        self.instance = Some(restored.instance);
        self.bound = true;
        Ok(())
    }

    fn rebind_for_bytes(&self, bytes: &[u8]) -> Result<Vec<HostRebind>, HostError> {
        let decoded = tollana_core::decode_container(bytes, None)?;
        let core = tollana_core::decode_tirs(&decoded.body.tirs)?;
        let mut rebind = Vec::new();
        for ident in &core.plugin_identities {
            let slot = self
                .slots
                .iter()
                .find(|s| s.hash == ident.identity_hash)
                .ok_or_else(|| {
                    HostError::new(format!("missing plugin for snapshot {}", ident.name))
                })?;
            let methods: Vec<(u32, u32)> = slot
                .plugin
                .methods()?
                .into_iter()
                .map(|(method_id, _)| (ident.plugin_id, method_id))
                .collect();
            rebind.push(HostRebind {
                plugin_id: ident.plugin_id,
                identity_hash: ident.identity_hash,
                name: ident.name.clone(),
                version: ident.version.clone(),
                methods,
            });
        }
        Ok(rebind)
    }
}

fn hash_plugin(plugin: &dyn Plugin) -> Result<[u8; 32], HostError> {
    Ok(hash_plugin_identity(&PluginIdentityInput {
        name: plugin.name(),
        version: plugin.version(),
        schema: plugin.schema(),
        metadata: plugin.metadata(),
        implementation_digest: None,
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, METHOD_NOW_MONOTONIC, METHOD_NOW_WALL};
    use crate::schema::CLOCK_SCHEMA_BYTES;
    use tollana_core::{CapHandle, Label};

    struct Stub {
        name: &'static str,
        schema: Vec<u8>,
    }

    impl Plugin for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn schema(&self) -> &[u8] {
            &self.schema
        }
        fn methods(&self) -> Result<Vec<(u32, tollana_core::FunctionType)>, HostError> {
            Ok(vec![(
                0,
                tollana_core::FunctionType {
                    parameters: vec![],
                    results: vec![tollana_core::ValueType::I32],
                },
            )])
        }
        fn invoke(
            &mut self,
            _method_id: u32,
            _args: &[Value],
            _caps: &[CapHandle],
        ) -> Result<PluginResult, HostError> {
            Ok(PluginResult::Immediate(vec![Value::i32(1, Label::Public)]))
        }
        fn snapshot_state(&self) -> Vec<u8> {
            Vec::new()
        }
        fn restore_state(&mut self, _bytes: &[u8]) -> Result<(), HostError> {
            Ok(())
        }
    }

    fn clock_module(plugin_id: u32, method_id: u32, import: &str) -> String {
        format!(
            r#"
(module
  (host.import {import}
    (pluginId {plugin_id})
    (methodId {method_id})
    (result i64))
  (func (export "main") (result i64)
    (host.invoke {import})))
"#
        )
    }

    #[test]
    fn assigned_ids_follow_sort_by_hash() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(0, 0))).unwrap();
        host.register(Box::new(Stub {
            name: "stub",
            schema: b"(schema stub v1)".to_vec(),
        }))
        .unwrap();
        host.bind().unwrap();
        let clock_id = host.plugin_id("clock").unwrap();
        let stub_id = host.plugin_id("stub").unwrap();
        assert_ne!(clock_id, stub_id);
        let clock_hash = hash_plugin(&Clock::virtual_at(0, 0)).unwrap();
        let stub_hash = hash_plugin_identity(&PluginIdentityInput {
            name: "stub",
            version: "1.0.0",
            schema: b"(schema stub v1)",
            metadata: b"",
            implementation_digest: None,
        })
        .unwrap();
        let ids = assign_local_ids(&[clock_hash, stub_hash]).unwrap();
        assert_eq!(clock_id, ids[0]);
        assert_eq!(stub_id, ids[1]);
    }

    #[test]
    fn guest_invokes_clock_by_assigned_id() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(1_700_000_000_000, 10)))
            .unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("clock").expect("clock id from sort-by-hash");
        host.instantiate_text(&clock_module(id, METHOD_NOW_WALL, "clock.now_wall"))
            .unwrap();
        let out = host.run("main", &[], 1000).unwrap();
        match out {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(1_700_000_000_000, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let samples = host.plugin_samples("clock");
        assert_eq!(samples, [(METHOD_NOW_WALL, 1_700_000_000_000)]);
        assert_eq!(
            host.instance()
                .unwrap()
                .plugin_identities()
                .iter()
                .any(|p| p.plugin_id == 0 && p.name == "clock"),
            id == 0
        );
        assert_eq!(
            host.instance().unwrap().plugin_identities()[0].plugin_id,
            id
        );
        assert_eq!(
            host.instance().unwrap().plugin_identities()[0].name,
            "clock"
        );
        let expected_hash = hash_plugin_identity(&PluginIdentityInput {
            name: "clock",
            version: "1.0.0",
            schema: CLOCK_SCHEMA_BYTES,
            metadata: b"",
            implementation_digest: None,
        })
        .unwrap();
        assert_eq!(
            host.instance().unwrap().plugin_identities()[0].identity_hash,
            expected_hash
        );
    }

    #[test]
    fn snapshot_restore_rebinds_clock_state_via_identity_map() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(42_000, 7)))
            .unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("clock").unwrap();
        host.instantiate_text(&clock_module(id, METHOD_NOW_WALL, "clock.now_wall"))
            .unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(42_000, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let bytes = host.snapshot().unwrap();
        let prior_journal_len = host.instance().unwrap().journal.events.len();
        assert!(prior_journal_len > 2);

        let mut host2 = Host::new();
        host2.register(Box::new(Clock::virtual_at(0, 0))).unwrap();
        host2.bind().unwrap();
        host2.restore(&bytes).unwrap();
        assert_eq!(host2.plugin_id("clock"), Some(id));
        assert_eq!(
            host2.instance().unwrap().plugin_identities()[0].plugin_id,
            id
        );
        assert_eq!(
            host2.instance().unwrap().journal.event_names(),
            ["SnapshotCoreRestored", "SnapshotRestored"]
        );
        assert!(host2.instance().unwrap().journal.events.len() < prior_journal_len);
        match host2.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(42_000, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn snapshot_restore_preserves_monotonic_virtual_clock() {
        let mut host = Host::new();
        host.register(Box::new(Clock::virtual_at(1, 77))).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("clock").unwrap();
        host.instantiate_text(&clock_module(
            id,
            METHOD_NOW_MONOTONIC,
            "clock.now_monotonic",
        ))
        .unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(77, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let bytes = host.snapshot().unwrap();
        let mut host2 = Host::new();
        host2.register(Box::new(Clock::virtual_at(0, 0))).unwrap();
        host2.restore(&bytes).unwrap();
        match host2.run("main", &[], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i64(77, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }
}
