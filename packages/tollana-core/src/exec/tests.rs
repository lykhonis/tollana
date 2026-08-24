use super::*;
use crate::decode::{decode_text, FunctionType, Module};
use crate::identity::{assign_local_ids, hash_plugin_identity, PluginIdentityInput};
use crate::journal::{JournalEventKind, JournalSink};
use crate::machine::{ControlLabelKind, QuotaDimension, QuotaSlot};
use crate::snapshot::{HostRebind, PluginIdentity};
use crate::value::{CapHandle, Label, ValueType};
use std::collections::HashMap;

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
    assert!(inst.machine.pending_host_calls.is_empty());
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

#[test]
fn program_4a_aligned_store_load() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.store (i32.const 0) (i32.const 0x01020304))
    (i32.load (i32.const 0))))
"#;
    let (inst, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(0x01020304, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(&inst.machine.linear_memory[0..4], &[0x04, 0x03, 0x02, 0x01]);
    assert_eq!(inst.machine.linear_memory.len(), 65536);
}

#[test]
fn program_4b_unaligned_store_load() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.store (i32.const 1) (i32.const 0x01020304))
    (i32.load (i32.const 1))))
"#;
    let (inst, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(0x01020304, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.linear_memory[0], 0);
    assert_eq!(&inst.machine.linear_memory[1..5], &[0x04, 0x03, 0x02, 0x01]);
}

#[test]
fn program_5_oob_load_traps() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    (i32.load (i32.const 65535))))
"#;
    let (inst, out) = run(src, 1000);
    match out {
        ExecOutcome::Trapped {
            trap_kind: TrapKind::OutOfBoundsMemory,
            program_counter,
        } => {
            assert_eq!(program_counter.instruction_index, 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 998);
    assert!(inst.machine.pending_host_calls.is_empty());
}

#[test]
fn if_else_and_br_to_function() {
    let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 1
    (if (result i32) (i32.const 10) else (i32.const 20))))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(10, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 0
    (if (result i32) (i32.const 10) else (i32.const 20))))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(20, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    let src = r#"
(module
  (func (export "main") (result i32)
    i32.const 7
    br 0))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(7, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    let src = r#"
(module
  (func (export "main") (result i32)
    (block
      i32.const 9
      return)
    i32.const 1))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(9, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn memory_size_is_page_count() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32)
    memory.size))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(1, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn i64_store_load() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i64)
    (i64.store (i32.const 8) (i64.const 0x0102030405060708))
    (i64.load (i32.const 8))))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i64(0x0102030405060708, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn loop_counts_to_three() {
    let src = r#"
(module
  (func (export "main") (result i32)
    (local i32)
    i32.const 0
    local.set 0
    (block
      (loop
        local.get 0
        i32.const 3
        i32.ge_s
        br_if 1
        local.get 0
        i32.const 1
        i32.add
        local.set 0
        br 0))
    local.get 0))
"#;
    let (_, out) = run(src, 10000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(3, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn call_adds() {
    let src = r#"
(module
  (func (export "main") (result i32)
    (call 1 (i32.const 2) (i32.const 3)))
  (func (param i32) (param i32) (result i32)
    local.get 0
    local.get 1
    i32.add))
"#;
    let (_, out) = run(src, 1000);
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(5, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

fn package_identity(name: &str, plugin_id: u32) -> PluginIdentity {
    let schema = format!("(schema {name} v1)");
    let hash = hash_plugin_identity(&PluginIdentityInput {
        name,
        version: "1.0.0",
        schema: schema.as_bytes(),
        metadata: b"",
        implementation_digest: None,
    })
    .unwrap();
    PluginIdentity {
        plugin_id,
        identity_hash: hash,
        name: name.to_string(),
        version: "1.0.0".into(),
    }
}

fn fixture_bindings(module: &Module) -> Vec<PluginBinding> {
    let mut order = Vec::new();
    let mut methods: HashMap<u32, Vec<(u32, FunctionType)>> = HashMap::new();
    let mut names: HashMap<u32, String> = HashMap::new();
    for imp in &module.host_imports {
        if names.insert(imp.plugin_id, imp.name.clone()).is_none() {
            order.push(imp.plugin_id);
        }
        let ty = module.types[imp.type_index as usize].clone();
        methods
            .entry(imp.plugin_id)
            .or_default()
            .push((imp.method_id, ty));
    }
    order
        .into_iter()
        .map(|plugin_id| PluginBinding {
            identity: package_identity(&names[&plugin_id], plugin_id),
            methods: methods.remove(&plugin_id).unwrap(),
        })
        .collect()
}

fn with_echo(src: &str) -> Instance {
    with_echo_quotas(src, &[])
}

fn with_echo_quotas(src: &str, quotas: &[QuotaSlot]) -> Instance {
    let module = decode_text(src).unwrap();
    let plugins = fixture_bindings(&module);
    Instance::instantiate_with(module, &plugins, Vec::new(), 16, quotas).unwrap()
}

fn i32_method() -> FunctionType {
    FunctionType {
        parameters: vec![ValueType::I32],
        results: vec![ValueType::I32],
    }
}

fn rebind_from(inst: &Instance) -> Vec<HostRebind> {
    inst.plugin_identities
        .iter()
        .map(|id| HostRebind {
            plugin_id: id.plugin_id,
            identity_hash: id.identity_hash,
            name: id.name.clone(),
            version: id.version.clone(),
            methods: inst
                .plugins
                .iter()
                .copied()
                .filter(|(plugin_id, _)| *plugin_id == id.plugin_id)
                .collect(),
        })
        .collect()
}

#[test]
fn program_2_echo() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;
    let mut inst = with_echo(src);
    let out = inst.invoke("main", &[], 1000).unwrap();
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 998);
    let call = inst.machine.pending_host_calls.first().unwrap();
    assert_eq!(call.arguments, vec![Value::i32(41, Label::Public)]);
    assert_eq!(
        inst.machine.continuations[0].call_frames[0].instruction_index,
        2
    );
    let out = inst.resume(0, vec![Value::i32(41, Label::Public)]).unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(42, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 995);
    inst = with_echo(src);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    assert_eq!(
        inst.continue_run(),
        Err(HostInterfaceError::HostCallPending)
    );
    let out = inst.trap_pending(0, TrapKind::HostTypeMismatch).unwrap();
    match out {
        ExecOutcome::Trapped {
            trap_kind: TrapKind::HostTypeMismatch,
            ..
        } => {}
        other => panic!("{other:?}"),
    }
    assert!(inst.machine.pending_host_calls.is_empty());
}

#[test]
fn program_7_capability_round_trip() {
    let src = r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke UseCapability (local.get 0))))
"#;
    let mut inst = with_echo(src);
    let handle = CapHandle {
        table_index: 1,
        generation: 1,
    };
    inst.grant_cap(handle, b"echo-cap".to_vec());
    let cap = Value::capability(handle, Label::Confidential);
    let out = inst.invoke("main", &[cap], 1000).unwrap();
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
    let call = inst.machine.pending_host_calls.first().unwrap();
    assert_eq!(call.arguments[0], cap);
    assert_eq!(call.capabilities, vec![handle]);
    let out = inst.resume(0, vec![Value::i32(7, Label::Public)]).unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(7, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn null_capability_traps_on_invoke() {
    let src = r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (result i32)
    (local Capability)
    (host.invoke UseCapability (local.get 0))))
"#;
    let mut inst = with_echo(src);
    let out = inst.invoke("main", &[], 1000).unwrap();
    match out {
        ExecOutcome::Trapped {
            trap_kind: TrapKind::InvalidCapability,
            program_counter,
        } => {
            assert_eq!(program_counter.instruction_index, 1);
        }
        other => panic!("{other:?}"),
    }
    assert!(inst.machine.pending_host_calls.is_empty());
}

#[test]
fn echo_suspend_snapshot_matches_tirs_hex() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let mut inst = with_echo(src);
    let fuel_before = inst.machine.remaining_fuel;
    let _ = inst.invoke("main", &[], 1000).unwrap();
    assert_eq!(inst.machine.remaining_fuel, 998);
    let snap = inst.snapshot_core();
    assert_eq!(inst.machine.remaining_fuel, 998);
    assert_eq!(fuel_before, 0);
    let bytes = crate::snapshot::encode_tirs(&snap);
    let again = crate::snapshot::decode_tirs(&bytes).unwrap();
    assert_eq!(again, snap);
    assert_eq!(snap.remaining_fuel, 998);
    assert_eq!(snap.continuations[0].call_frames[0].instruction_index, 2);
    assert_ne!(snap.plugin_identities[0].identity_hash, [0u8; 32]);
    assert_eq!(snap.plugin_identities[0].name, "Echo");
    assert_eq!(snap.plugin_identities[0].version, "1.0.0");
}

#[test]
fn program_3_snapshot_restore_resume() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;
    let mut inst = with_echo(src);
    let out = inst.invoke("main", &[], 1000).unwrap();
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
    let snap = inst.snapshot_core();
    assert_eq!(snap.module_bytes, inst.machine.module_bytes);
    assert_eq!(snap.remaining_fuel, 998);
    assert_eq!(snap.continuations[0].value_stack.len(), 0);
    assert_eq!(snap.continuations[0].call_frames[0].instruction_index, 2);
    assert_eq!(snap.continuations[0].call_frames[0].function_index, 0);
    let label = snap.continuations[0].call_frames[0].control_stack[0];
    assert_eq!(label.label_kind, ControlLabelKind::Block);
    assert_eq!(label.result_count, 1);
    assert_eq!(label.stack_height, 0);
    let call = snap.pending_host_calls.first().unwrap();
    assert_eq!(call.arguments, vec![Value::i32(41, Label::Public)]);
    assert_eq!(snap.linear_memory.len(), 0);
    assert!(snap.capability_table.is_empty());
    let rebind = rebind_from(&inst);
    drop(inst);
    let mut restored = Instance::restore_core(snap, &rebind, None).unwrap();
    assert_eq!(restored.machine.remaining_fuel, 998);
    let out = restored
        .resume(0, vec![Value::i32(41, Label::Public)])
        .unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(42, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(restored.machine.remaining_fuel, 995);
}

#[test]
fn program_3_via_container_bytes() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;
    let mut inst = with_echo(src);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let fuel = inst.machine.remaining_fuel;
    let bytes = inst.snapshot(Vec::new());
    assert_eq!(inst.machine.remaining_fuel, fuel);
    let cursor = inst.journal.next_sequence();
    let rebind = rebind_from(&inst);
    drop(inst);
    let restored = Instance::restore(&bytes, &rebind, None, None).unwrap();
    assert!(restored.plugin_state.is_empty());
    assert_eq!(restored.journal_cursor, cursor);
    let mut inst = restored.instance;
    assert_eq!(inst.machine.remaining_fuel, 998);
    let out = inst.resume(0, vec![Value::i32(41, Label::Public)]).unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(42, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 995);
}

#[test]
fn container_bit_flip_rejects_restore() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let mut inst = with_echo(src);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let mut bytes = inst.snapshot(Vec::new());
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    let rebind = rebind_from(&inst);
    match Instance::restore(&bytes, &rebind, None, None) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(
                message.contains("checksum")
                    || message.contains("mismatch")
                    || message.contains("end of")
            );
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn program_3_via_aead_container() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;
    let mut inst = with_echo(src);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let key = [0x11u8; 32];
    let nonce = [0x22u8; 12];
    let bytes = inst.snapshot_aead(Vec::new(), &key, &nonce).unwrap();
    let rebind = rebind_from(&inst);
    drop(inst);
    match Instance::restore(&bytes, &rebind, Some(&[0x33u8; 32]), None) {
        Err(HostInterfaceError::Reject { .. }) => {}
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
    let restored = Instance::restore(&bytes, &rebind, Some(&key), None).unwrap();
    let mut inst = restored.instance;
    let out = inst.resume(0, vec![Value::i32(41, Label::Public)]).unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(42, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 995);
}

#[test]
fn restore_rejects_identity_hash_mismatch() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let mut inst = with_echo(src);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let snap = inst.snapshot_core();
    let mut rebind = rebind_from(&inst);
    rebind[0].identity_hash[0] ^= 0xff;
    match Instance::restore_core(snap, &rebind, None) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("identity"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn restore_rejects_capability_generation_mismatch() {
    let src = r#"
(module
  (host.import UseCapability
    (pluginId 0)
    (methodId 0)
    (param Capability)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke UseCapability (local.get 0))))
"#;
    let mut inst = with_echo(src);
    let handle = CapHandle {
        table_index: 1,
        generation: 1,
    };
    inst.grant_cap(handle, b"echo-cap".to_vec());
    let cap = Value::capability(handle, Label::Confidential);
    let _ = inst.invoke("main", &[cap], 1000).unwrap();
    let rebind = rebind_from(&inst);
    let mut snap = inst.snapshot_core();
    snap.capability_table[0].generation = 2;
    match Instance::restore_core(snap, &rebind, None) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("capability"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn instantiate_rejects_missing_plugin_binding() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let module = decode_text(src).unwrap();
    match Instance::instantiate_with(module, &[], Vec::new(), 16, &[]) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("missing plugin"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn instantiate_rejects_method_type_mismatch() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let module = decode_text(src).unwrap();
    let plugins = [PluginBinding {
        identity: package_identity("Echo", 0),
        methods: vec![(
            0,
            FunctionType {
                parameters: vec![ValueType::I64],
                results: vec![ValueType::I32],
            },
        )],
    }];
    match Instance::instantiate_with(module, &plugins, Vec::new(), 16, &[]) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("type mismatch"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn instantiate_does_not_renumber_plugin_ids() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 7)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#;
    let module = decode_text(src).unwrap();
    let plugins = fixture_bindings(&module);
    let mut inst = Instance::instantiate_with(module, &plugins, Vec::new(), 16, &[]).unwrap();
    let snap = inst.snapshot_core();
    assert_eq!(snap.plugin_identities[0].plugin_id, 7);
    assert_ne!(snap.plugin_identities[0].identity_hash, [0u8; 32]);
    assert_eq!(snap.plugin_identities[0].name, "Echo");
    assert_eq!(snap.plugin_identities[0].version, "1.0.0");
}

#[test]
fn two_package_sort_by_hash_restore_rejects_v2() {
    let echo = PluginIdentityInput {
        name: "echo",
        version: "1.0.0",
        schema: b"(schema echo v1)",
        metadata: b"",
        implementation_digest: None,
    };
    let echo_v2 = PluginIdentityInput {
        version: "2.0.0",
        ..echo
    };
    let clock = PluginIdentityInput {
        name: "clock",
        version: "1.0.0",
        schema: b"(schema clock v1)",
        metadata: b"",
        implementation_digest: None,
    };
    let echo_hash = hash_plugin_identity(&echo).unwrap();
    let echo_v2_hash = hash_plugin_identity(&echo_v2).unwrap();
    let clock_hash = hash_plugin_identity(&clock).unwrap();
    let ids = assign_local_ids(&[echo_hash, clock_hash]).unwrap();
    assert_eq!(ids, [1, 0]);
    let echo_id = ids[0];
    let clock_id = ids[1];
    let src = format!(
        r#"
(module
  (host.import Echo
    (pluginId {echo_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import Clock
    (pluginId {clock_id})
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (host.invoke Echo (i32.const 41))))
"#
    );
    let module = decode_text(&src).unwrap();
    let plugins = [
        PluginBinding {
            identity: PluginIdentity {
                plugin_id: echo_id,
                identity_hash: echo_hash,
                name: "echo".into(),
                version: "1.0.0".into(),
            },
            methods: vec![(0, i32_method())],
        },
        PluginBinding {
            identity: PluginIdentity {
                plugin_id: clock_id,
                identity_hash: clock_hash,
                name: "clock".into(),
                version: "1.0.0".into(),
            },
            methods: vec![(0, i32_method())],
        },
    ];
    let mut inst = Instance::instantiate_with(module, &plugins, Vec::new(), 16, &[]).unwrap();
    let snap = inst.snapshot_core();
    assert_eq!(snap.plugin_identities.len(), 2);
    assert_eq!(snap.plugin_identities[0].plugin_id, echo_id);
    assert_eq!(snap.plugin_identities[0].identity_hash, echo_hash);
    assert_eq!(snap.plugin_identities[1].plugin_id, clock_id);
    assert_eq!(snap.plugin_identities[1].identity_hash, clock_hash);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let snap = inst.snapshot_core();
    let matching = [
        HostRebind {
            plugin_id: echo_id,
            identity_hash: echo_hash,
            name: "echo".into(),
            version: "1.0.0".into(),
            methods: vec![(echo_id, 0)],
        },
        HostRebind {
            plugin_id: clock_id,
            identity_hash: clock_hash,
            name: "clock".into(),
            version: "1.0.0".into(),
            methods: vec![(clock_id, 0)],
        },
    ];
    Instance::restore_core(snap.clone(), &matching, None).expect("matching hashes restore");
    let mut rebound = matching.clone();
    rebound[0].identity_hash = echo_v2_hash;
    rebound[0].version = "2.0.0".into();
    match Instance::restore_core(snap, &rebound, None) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("identity"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject of echo v2"),
    }
}

const PROGRAM_2: &str = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 41))
      (i32.const 1))))
"#;

#[test]
fn program_2_journal_order_same_process_restore() {
    let mut inst = with_echo(PROGRAM_2);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    assert_eq!(
        inst.journal.event_names(),
        ["InstanceCreated", "HostCallSuspended"]
    );
    let bytes = inst.snapshot(Vec::new());
    assert_eq!(
        inst.journal.event_names(),
        [
            "InstanceCreated",
            "HostCallSuspended",
            "SnapshotCoreTaken",
            "SnapshotTaken"
        ]
    );
    assert!(!inst.journal.event_names().contains(&"InstructionStepped"));
    let seqs: Vec<u64> = inst.journal.events.iter().map(|e| e.sequence).collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
    let cursor = inst.journal.next_sequence();
    let journal = inst.journal.clone();
    let rebind = rebind_from(&inst);
    let mut restored = Instance::restore(&bytes, &rebind, None, Some(journal)).unwrap();
    assert_eq!(restored.journal_cursor, cursor);
    let names = restored.instance.journal.event_names();
    assert_eq!(
        names,
        [
            "InstanceCreated",
            "HostCallSuspended",
            "SnapshotCoreTaken",
            "SnapshotTaken",
            "SnapshotCoreRestored",
            "SnapshotRestored"
        ]
    );
    let seqs: Vec<u64> = restored
        .instance
        .journal
        .events
        .iter()
        .map(|e| e.sequence)
        .collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
    let out = restored
        .instance
        .resume(0, vec![Value::i32(41, Label::Public)])
        .unwrap();
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(42, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        restored.instance.journal.event_names(),
        [
            "InstanceCreated",
            "HostCallSuspended",
            "SnapshotCoreTaken",
            "SnapshotTaken",
            "SnapshotCoreRestored",
            "SnapshotRestored",
            "HostCallResumed",
            "Completed"
        ]
    );
    assert_eq!(restored.instance.machine.remaining_fuel, 995);
}

#[test]
fn default_journal_redacts_confidential_host_call_args() {
    let src = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (param i32) (result i32)
    (host.invoke Echo (local.get 0))))
"#;
    let mut inst = with_echo(src);
    let _ = inst
        .invoke("main", &[Value::i32(41, Label::Confidential)], 1000)
        .unwrap();
    let suspended = inst
        .journal
        .events
        .iter()
        .find(|e| e.kind.name() == "HostCallSuspended")
        .unwrap();
    assert_eq!(suspended.sensitivity, Label::Confidential);
    match &suspended.kind {
        JournalEventKind::HostCallSuspended { arguments, .. } => {
            assert_eq!(arguments[0].label, Label::Confidential);
            assert_eq!(arguments[0].payload, None);
        }
        other => panic!("{}", other.name()),
    }
}

#[test]
fn restore_without_journal_starts_new_seq() {
    let mut inst = with_echo(PROGRAM_2);
    let _ = inst.invoke("main", &[], 1000).unwrap();
    let bytes = inst.snapshot(Vec::new());
    let rebind = rebind_from(&inst);
    let restored = Instance::restore(&bytes, &rebind, None, None).unwrap();
    assert_eq!(
        restored.instance.journal.event_names(),
        ["SnapshotCoreRestored", "SnapshotRestored"]
    );
    assert_eq!(restored.instance.journal.events[0].sequence, 0);
}

const TWO_ECHO: &str = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "main") (result i32)
    (i32.add
      (host.invoke Echo (i32.const 1))
      (host.invoke Echo (i32.const 2)))))
"#;

#[test]
fn host_call_quota_exhaust_snapshot_restore_add_quota() {
    let quotas = [QuotaSlot {
        dimension: QuotaDimension::HostCallCount,
        remaining: 1,
    }];
    let mut inst = with_echo_quotas(TWO_ECHO, &quotas);
    let out = inst.invoke("main", &[], 1000).unwrap();
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.quota_remaining(QuotaDimension::HostCallCount), Some(0));
    let fuel_after_first = inst.machine.remaining_fuel;
    assert_eq!(fuel_after_first, 998);
    let out = inst.resume(0, vec![Value::i32(10, Label::Public)]).unwrap();
    match out {
        ExecOutcome::Suspended {
            reason:
                SuspendReason::QuotaExhausted {
                    dimension: QuotaDimension::HostCallCount,
                },
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.remaining_fuel, 997);
    assert_eq!(inst.quota_remaining(QuotaDimension::HostCallCount), Some(0));
    let names = inst.journal.event_names();
    assert!(names.contains(&"QuotaConsumed"));
    assert!(names.contains(&"QuotaExhausted"));
    assert!(!names.contains(&"QuotaAdded"));
    let snap = inst.snapshot_core();
    assert_eq!(inst.machine.remaining_fuel, 997);
    assert_eq!(
        snap.quotas,
        [QuotaSlot {
            dimension: QuotaDimension::HostCallCount,
            remaining: 0,
        }]
    );
    let rebind = rebind_from(&inst);
    let mut restored = Instance::restore_core(snap, &rebind, None).unwrap();
    assert_eq!(
        restored.quota_remaining(QuotaDimension::HostCallCount),
        Some(0)
    );
    assert_eq!(restored.machine.remaining_fuel, 997);
    match restored.continue_run().unwrap() {
        ExecOutcome::Suspended {
            reason:
                SuspendReason::QuotaExhausted {
                    dimension: QuotaDimension::HostCallCount,
                },
        } => {}
        other => panic!("{other:?}"),
    }
    restored.add_quota(QuotaDimension::HostCallCount, 1);
    assert_eq!(
        restored.quota_remaining(QuotaDimension::HostCallCount),
        Some(1)
    );
    match restored.continue_run().unwrap() {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(
        restored.quota_remaining(QuotaDimension::HostCallCount),
        Some(0)
    );
    match restored
        .resume(0, vec![Value::i32(20, Label::Public)])
        .unwrap()
    {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(30, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
    assert!(restored.journal.event_names().contains(&"QuotaAdded"));
}

#[test]
fn fuel_exhaustion_wins_over_quota() {
    let quotas = [QuotaSlot {
        dimension: QuotaDimension::HostCallCount,
        remaining: 0,
    }];
    let mut inst = with_echo_quotas(PROGRAM_2, &quotas);
    match inst.invoke("main", &[], 0).unwrap() {
        ExecOutcome::Suspended {
            reason: SuspendReason::OutOfFuel,
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn memory_quota_rejects_undersized_cap() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32) (i32.const 1)))
"#;
    let module = decode_text(src).unwrap();
    match Instance::instantiate_with(
        module,
        &[],
        Vec::new(),
        16,
        &[QuotaSlot {
            dimension: QuotaDimension::MemoryBytes,
            remaining: 10,
        }],
    ) {
        Err(HostInterfaceError::Reject { message }) => {
            assert!(message.contains("memory quota"));
        }
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected reject"),
    }
}

#[test]
fn memory_quota_remaining_after_allocate() {
    let src = r#"
(module
  (memory (pages 1))
  (func (export "main") (result i32) (i32.const 1)))
"#;
    let module = decode_text(src).unwrap();
    let inst = Instance::instantiate_with(
        module,
        &[],
        Vec::new(),
        16,
        &[QuotaSlot {
            dimension: QuotaDimension::MemoryBytes,
            remaining: 65536 * 2,
        }],
    )
    .unwrap();
    assert_eq!(
        inst.quota_remaining(QuotaDimension::MemoryBytes),
        Some(65536)
    );
}

const SIBLINGS: &str = r#"
(module
  (host.import Echo
    (pluginId 0)
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "a") (result i32)
    (host.invoke Echo (i32.const 1)))
  (func (export "b") (result i32)
    (host.invoke Echo (i32.const 2))))
"#;

fn assert_suspended_invoke(out: ExecOutcome) {
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        } => {}
        other => panic!("{other:?}"),
    }
}

fn assert_completed(out: ExecOutcome, bits: i32) {
    match out {
        ExecOutcome::Completed { results } => {
            assert_eq!(results, vec![Value::i32(bits, Label::Public)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn program_8_sibling_host_calls_snapshot_restore_resume() {
    let mut inst = with_echo(SIBLINGS);
    assert_suspended_invoke(inst.invoke("a", &[], 1000).unwrap());
    assert_eq!(inst.machine.remaining_fuel, 998);
    assert_eq!(inst.machine.continuations.len(), 1);
    assert_eq!(inst.machine.pending_host_calls.len(), 1);
    assert_eq!(
        inst.machine.pending_host_calls[0].continuation_identifier,
        0
    );
    assert_eq!(
        inst.invoke("a", &[], 1000),
        Err(HostInterfaceError::Reject {
            message: "instance has live continuations".into(),
        })
    );
    let fuel_after_first = inst.machine.remaining_fuel;
    let (id_b, out) = inst.spawn_continuation("b", &[]).unwrap();
    assert_eq!(id_b, 1);
    assert_suspended_invoke(out);
    assert_eq!(inst.machine.remaining_fuel, fuel_after_first - 2);
    assert_eq!(inst.machine.remaining_fuel, 996);
    assert_eq!(inst.machine.continuations.len(), 2);
    assert_eq!(inst.machine.pending_host_calls.len(), 2);
    let mut pending_ids: Vec<u32> = inst
        .machine
        .pending_host_calls
        .iter()
        .map(|c| c.continuation_identifier)
        .collect();
    pending_ids.sort_unstable();
    assert_eq!(pending_ids, [0, 1]);
    assert_eq!(
        inst.continue_run(),
        Err(HostInterfaceError::HostCallPending)
    );

    let snap = inst.snapshot_core();
    assert_eq!(snap.continuations.len(), 2);
    assert_eq!(snap.pending_host_calls.len(), 2);
    assert_eq!(snap.remaining_fuel, 996);
    let bytes = crate::snapshot::encode_tirs(&snap);
    let decoded = crate::snapshot::decode_tirs(&bytes).unwrap();
    assert_eq!(decoded, snap);

    let container = inst.snapshot(Vec::new());
    let rebind = rebind_from(&inst);
    drop(inst);
    let restored = Instance::restore(&container, &rebind, None, None).unwrap();
    let mut inst = restored.instance;
    assert_eq!(inst.machine.remaining_fuel, 996);
    assert_eq!(inst.machine.continuations.len(), 2);
    assert_eq!(inst.machine.pending_host_calls.len(), 2);

    assert_completed(
        inst.resume(0, vec![Value::i32(41, Label::Public)]).unwrap(),
        41,
    );
    assert_eq!(inst.machine.remaining_fuel, 995);
    assert_eq!(inst.machine.continuations.len(), 1);
    assert_eq!(inst.machine.continuations[0].continuation_identifier, 1);
    assert_eq!(
        inst.continue_run(),
        Err(HostInterfaceError::HostCallPending)
    );
    assert_completed(
        inst.resume(1, vec![Value::i32(42, Label::Public)]).unwrap(),
        42,
    );
    assert_eq!(inst.machine.remaining_fuel, 994);
    assert!(inst.machine.continuations.is_empty());
    assert!(inst.machine.pending_host_calls.is_empty());
}

#[test]
fn program_8_resume_higher_id_first() {
    let mut inst = with_echo(SIBLINGS);
    assert_suspended_invoke(inst.invoke("a", &[], 1000).unwrap());
    let (id_b, out) = inst.spawn_continuation("b", &[]).unwrap();
    assert_eq!(id_b, 1);
    assert_suspended_invoke(out);
    let fuel = inst.machine.remaining_fuel;
    assert_completed(
        inst.resume(1, vec![Value::i32(42, Label::Public)]).unwrap(),
        42,
    );
    assert_eq!(inst.machine.remaining_fuel, fuel - 1);
    assert_eq!(inst.machine.continuations.len(), 1);
    assert_eq!(inst.machine.continuations[0].continuation_identifier, 0);
    assert_completed(
        inst.resume(0, vec![Value::i32(41, Label::Public)]).unwrap(),
        41,
    );
    assert_eq!(inst.machine.remaining_fuel, fuel - 2);
}

#[test]
fn continue_runs_lowest_ready_id() {
    let src = r#"
(module
  (func (export "a") (result i32) (i32.const 1))
  (func (export "b") (result i32) (i32.const 2)))
"#;
    let module = decode_text(src).unwrap();
    let mut inst = Instance::instantiate(module).unwrap();
    match inst.invoke("a", &[], 0).unwrap() {
        ExecOutcome::Suspended {
            reason: SuspendReason::OutOfFuel,
        } => {}
        other => panic!("{other:?}"),
    }
    let (id_b, out) = inst.spawn_continuation("b", &[]).unwrap();
    assert_eq!(id_b, 1);
    match out {
        ExecOutcome::Suspended {
            reason: SuspendReason::OutOfFuel,
        } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(inst.machine.continuations.len(), 2);
    inst.add_fuel(10);
    assert_completed(inst.continue_run().unwrap(), 1);
    assert_eq!(inst.machine.continuations.len(), 1);
    assert_eq!(inst.machine.continuations[0].continuation_identifier, 1);
    assert_completed(inst.continue_run().unwrap(), 2);
}
