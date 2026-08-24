use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, GOAL_SCHEMA_BYTES};
use std::collections::HashMap;
use tollana_core::{CapHandle, ExecOutcome, FunctionType, Label, QuotaDimension, Value};

pub const METHOD_SPAWN: u32 = 0;
pub const METHOD_JOIN: u32 = 1;
pub const METHOD_CANCEL: u32 = 2;

#[derive(Clone, Debug)]
pub struct Railguards {
    pub max_depth: u32,
    pub max_concurrent: u32,
    pub max_children: u32,
    pub approval_beyond_depth: Option<u32>,
    pub timeout_millis: Option<u64>,
    pub attenuate_children: bool,
    pub child_host_call_slice: Option<u64>,
}

impl Default for Railguards {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_concurrent: 32,
            max_children: 16,
            approval_beyond_depth: None,
            timeout_millis: None,
            attenuate_children: false,
            child_host_call_slice: None,
        }
    }
}

#[derive(Clone, Debug)]
enum GoalStatus {
    Running,
    Completed(i32),
    Cancelled,
}

#[derive(Clone, Debug)]
struct GoalNode {
    id: u32,
    continuation_id: u32,
    parent_id: Option<u32>,
    depth: u32,
    children: Vec<u32>,
    status: GoalStatus,
    allowed_caps: Option<Vec<CapHandle>>,
    host_call_budget: Option<u64>,
    charged_concurrent: bool,
    deadline_millis: Option<u64>,
}

pub struct Goal {
    railguards: Railguards,
    nodes: HashMap<u32, GoalNode>,
    by_continuation: HashMap<u32, u32>,
    next_id: u32,
    now_millis: u64,
    pending_releases: u64,
}

impl Default for Goal {
    fn default() -> Self {
        Self::new()
    }
}

impl Goal {
    pub fn new() -> Self {
        Self {
            railguards: Railguards::default(),
            nodes: HashMap::new(),
            by_continuation: HashMap::new(),
            next_id: 1,
            now_millis: 0,
            pending_releases: 0,
        }
    }

    pub fn with_railguards(mut self, railguards: Railguards) -> Self {
        self.railguards = railguards;
        self
    }

    pub fn set_now_millis(&mut self, now: u64) {
        self.now_millis = now;
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn running_children(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.parent_id.is_some() && matches!(n.status, GoalStatus::Running))
            .count()
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn ensure_root(
        &mut self,
        caller: u32,
        parent_caps: Option<Vec<CapHandle>>,
    ) -> Result<u32, HostError> {
        if let Some(id) = self.by_continuation.get(&caller).copied() {
            return Ok(id);
        }
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            GoalNode {
                id,
                continuation_id: caller,
                parent_id: None,
                depth: 0,
                children: Vec::new(),
                status: GoalStatus::Running,
                allowed_caps: parent_caps,
                host_call_budget: None,
                charged_concurrent: false,
                deadline_millis: None,
            },
        );
        self.by_continuation.insert(caller, id);
        Ok(id)
    }

    fn deny(reason: &str) -> Result<PluginResult, HostError> {
        Err(HostError::new(reason))
    }

    fn spawn(
        &mut self,
        function_index: i32,
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        if function_index < 0 {
            return Self::deny("invalid_function");
        }
        let caller = ctx.caller_continuation();
        let live = ctx.live_capabilities();
        let parent_id = self.ensure_root(caller, Some(live.clone()))?;
        let parent_depth = self.nodes[&parent_id].depth;
        let child_depth = parent_depth + 1;
        if child_depth > self.railguards.max_depth {
            return Self::deny("max_depth");
        }
        if self
            .railguards
            .approval_beyond_depth
            .is_some_and(|d| child_depth > d)
        {
            return Self::deny("approval");
        }
        let child_count = self.nodes[&parent_id].children.len() as u32;
        if child_count >= self.railguards.max_children {
            return Self::deny("max_children");
        }
        if self.running_children() as u32 >= self.railguards.max_concurrent {
            return Self::deny("max_concurrent");
        }
        let mut charged = false;
        if ctx
            .quota_remaining(QuotaDimension::ConcurrentGoals)
            .is_some()
        {
            if !ctx.consume_quota(QuotaDimension::ConcurrentGoals, 1) {
                return Self::deny("concurrent_goals_quota");
            }
            charged = true;
        }
        let export = ctx.function_export_name(function_index as u32)?;
        let (continuation_id, outcome) = ctx.spawn_export(&export, &[])?;
        let parent_allowed = self.nodes[&parent_id].allowed_caps.clone();
        let allowed_caps = if self.railguards.attenuate_children {
            Some(Vec::new())
        } else {
            parent_allowed.or(Some(live))
        };
        let host_call_budget = self.railguards.child_host_call_slice;
        let deadline_millis = self
            .railguards
            .timeout_millis
            .map(|t| self.now_millis.saturating_add(t));
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            GoalNode {
                id,
                continuation_id,
                parent_id: Some(parent_id),
                depth: child_depth,
                children: Vec::new(),
                status: GoalStatus::Running,
                allowed_caps: allowed_caps.clone(),
                host_call_budget,
                charged_concurrent: charged,
                deadline_millis,
            },
        );
        self.by_continuation.insert(continuation_id, id);
        self.nodes.get_mut(&parent_id).unwrap().children.push(id);
        if let ExecOutcome::Completed { results } = outcome {
            self.finish(id, ctx, GoalStatus::Completed(result_i32(&results)));
        }
        Ok(PluginResult::Immediate(vec![Value::i32(
            id as i32,
            Label::Public,
        )]))
    }

    fn join(
        &mut self,
        goal_id: i32,
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        if goal_id <= 0 {
            return Err(HostError::new("invalid goal id"));
        }
        let id = goal_id as u32;
        self.expire_if_needed(id, ctx)?;
        let node = self
            .nodes
            .get(&id)
            .ok_or_else(|| HostError::new("unknown goal"))?;
        match node.status {
            GoalStatus::Running => Ok(PluginResult::Pending(id as u64)),
            GoalStatus::Completed(v) => {
                Ok(PluginResult::Immediate(vec![Value::i32(v, Label::Public)]))
            }
            GoalStatus::Cancelled => {
                Ok(PluginResult::Immediate(vec![Value::i32(0, Label::Public)]))
            }
        }
    }

    fn cancel(
        &mut self,
        goal_id: i32,
        ctx: &mut dyn PluginContext,
    ) -> Result<PluginResult, HostError> {
        if goal_id <= 0 {
            return Err(HostError::new("invalid goal id"));
        }
        self.cancel_tree(goal_id as u32, ctx)?;
        Ok(PluginResult::Immediate(vec![Value::unit(Label::Public)]))
    }

    fn expire_if_needed(&mut self, id: u32, ctx: &mut dyn PluginContext) -> Result<(), HostError> {
        let expired = self.nodes.get(&id).is_some_and(|n| {
            matches!(n.status, GoalStatus::Running)
                && n.deadline_millis.is_some_and(|d| self.now_millis > d)
        });
        if expired {
            self.cancel_tree(id, ctx)?;
        }
        Ok(())
    }

    fn cancel_tree(&mut self, id: u32, ctx: &mut dyn PluginContext) -> Result<(), HostError> {
        let children = self
            .nodes
            .get(&id)
            .map(|n| n.children.clone())
            .ok_or_else(|| HostError::new("unknown goal"))?;
        for child in children {
            self.cancel_tree(child, ctx)?;
        }
        let (continuation_id, charged, running) = {
            let node = self
                .nodes
                .get_mut(&id)
                .ok_or_else(|| HostError::new("unknown goal"))?;
            if matches!(node.status, GoalStatus::Running) {
                node.status = GoalStatus::Cancelled;
                (node.continuation_id, node.charged_concurrent, true)
            } else {
                (0, false, false)
            }
        };
        if running {
            ctx.cancel_continuation(continuation_id)?;
            if charged {
                ctx.add_quota(QuotaDimension::ConcurrentGoals, 1);
            }
        }
        Ok(())
    }

    fn finish(&mut self, id: u32, ctx: &mut dyn PluginContext, status: GoalStatus) {
        let charged = match self.nodes.get_mut(&id) {
            Some(node) if matches!(node.status, GoalStatus::Running) => {
                let charged = node.charged_concurrent;
                node.charged_concurrent = false;
                node.status = status;
                charged
            }
            _ => return,
        };
        if charged {
            ctx.add_quota(QuotaDimension::ConcurrentGoals, 1);
        }
    }
}

fn result_i32(results: &[Value]) -> i32 {
    results.first().and_then(|v| v.as_i32()).unwrap_or(0)
}

impl Plugin for Goal {
    fn name(&self) -> &str {
        "goal"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        GOAL_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(GOAL_SCHEMA_BYTES)?;
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
            METHOD_SPAWN => {
                let idx = args
                    .first()
                    .and_then(|v| v.as_i32())
                    .ok_or_else(|| HostError::new("spawn expects i32 function index"))?;
                self.spawn(idx, ctx)
            }
            METHOD_JOIN => {
                let id = args
                    .first()
                    .and_then(|v| v.as_i32())
                    .ok_or_else(|| HostError::new("join expects i32 goal id"))?;
                self.join(id, ctx)
            }
            METHOD_CANCEL => {
                let id = args
                    .first()
                    .and_then(|v| v.as_i32())
                    .ok_or_else(|| HostError::new("cancel expects i32 goal id"))?;
                self.cancel(id, ctx)
            }
            other => Err(HostError::new(format!("unknown goal method {other}"))),
        }
    }

    fn on_continuation_completed(&mut self, continuation_id: u32, results: &[Value]) {
        let Some(id) = self.by_continuation.get(&continuation_id).copied() else {
            return;
        };
        let charged = match self.nodes.get_mut(&id) {
            Some(node) if matches!(node.status, GoalStatus::Running) => {
                let charged = node.charged_concurrent;
                node.charged_concurrent = false;
                node.status = GoalStatus::Completed(result_i32(results));
                charged
            }
            _ => return,
        };
        if charged {
            self.pending_releases += 1;
        }
    }

    fn take_quota_credits(&mut self) -> Vec<(QuotaDimension, u64)> {
        let n = self.pending_releases;
        self.pending_releases = 0;
        if n == 0 {
            Vec::new()
        } else {
            vec![(QuotaDimension::ConcurrentGoals, n)]
        }
    }

    fn capability_allowlist(&self, continuation_id: u32) -> Option<Vec<CapHandle>> {
        self.by_continuation
            .get(&continuation_id)
            .and_then(|id| self.nodes.get(id))
            .and_then(|n| n.allowed_caps.clone())
    }

    fn charge_host_call(&mut self, continuation_id: u32) -> Result<(), HostError> {
        let Some(id) = self.by_continuation.get(&continuation_id).copied() else {
            return Ok(());
        };
        let mut walk = vec![id];
        let mut parent = self.nodes.get(&id).and_then(|n| n.parent_id);
        while let Some(p) = parent {
            walk.push(p);
            parent = self.nodes.get(&p).and_then(|n| n.parent_id);
        }
        for nid in walk {
            if let Some(node) = self.nodes.get_mut(&nid) {
                if let Some(budget) = node.host_call_budget.as_mut() {
                    if *budget == 0 {
                        return Err(HostError::new("subtree host-call slice exhausted"));
                    }
                    *budget -= 1;
                }
            }
        }
        Ok(())
    }

    fn snapshot_state(&self) -> Vec<u8> {
        encode_goal(self)
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        *self = decode_goal(bytes)?;
        Ok(())
    }
}

fn encode_goal(goal: &Goal) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(1);
    put_u32(&mut buf, goal.railguards.max_depth);
    put_u32(&mut buf, goal.railguards.max_concurrent);
    put_u32(&mut buf, goal.railguards.max_children);
    put_u32(
        &mut buf,
        goal.railguards.approval_beyond_depth.unwrap_or(u32::MAX),
    );
    put_u64(&mut buf, goal.railguards.timeout_millis.unwrap_or(0));
    buf.push(u8::from(goal.railguards.attenuate_children));
    put_u64(
        &mut buf,
        goal.railguards.child_host_call_slice.unwrap_or(u64::MAX),
    );
    put_u32(&mut buf, goal.next_id);
    put_u64(&mut buf, goal.now_millis);
    put_u32(&mut buf, goal.nodes.len() as u32);
    let mut ids: Vec<u32> = goal.nodes.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let n = &goal.nodes[&id];
        put_u32(&mut buf, n.id);
        put_u32(&mut buf, n.continuation_id);
        put_u32(&mut buf, n.parent_id.unwrap_or(u32::MAX));
        put_u32(&mut buf, n.depth);
        match n.status {
            GoalStatus::Running => {
                buf.push(0);
                put_i32(&mut buf, 0);
            }
            GoalStatus::Completed(v) => {
                buf.push(1);
                put_i32(&mut buf, v);
            }
            GoalStatus::Cancelled => {
                buf.push(2);
                put_i32(&mut buf, 0);
            }
        }
        buf.push(u8::from(n.charged_concurrent));
        put_u64(&mut buf, n.host_call_budget.unwrap_or(u64::MAX));
        put_u64(&mut buf, n.deadline_millis.unwrap_or(0));
        put_u32(&mut buf, n.children.len() as u32);
        for c in &n.children {
            put_u32(&mut buf, *c);
        }
        match &n.allowed_caps {
            None => put_u32(&mut buf, u32::MAX),
            Some(caps) => {
                put_u32(&mut buf, caps.len() as u32);
                for h in caps {
                    put_u32(&mut buf, h.table_index);
                    put_u32(&mut buf, h.generation);
                }
            }
        }
    }
    buf
}

fn decode_goal(bytes: &[u8]) -> Result<Goal, HostError> {
    let mut r = Reader {
        data: bytes,
        pos: 0,
    };
    if r.u8()? != 1 {
        return Err(HostError::new("unsupported goal snapshot version"));
    }
    let max_depth = r.u32()?;
    let max_concurrent = r.u32()?;
    let max_children = r.u32()?;
    let approval = r.u32()?;
    let timeout = r.u64()?;
    let attenuate = r.u8()? != 0;
    let slice = r.u64()?;
    let next_id = r.u32()?;
    let now_millis = r.u64()?;
    let count = r.u32()? as usize;
    let mut goal = Goal {
        railguards: Railguards {
            max_depth,
            max_concurrent,
            max_children,
            approval_beyond_depth: if approval == u32::MAX {
                None
            } else {
                Some(approval)
            },
            timeout_millis: if timeout == 0 { None } else { Some(timeout) },
            attenuate_children: attenuate,
            child_host_call_slice: if slice == u64::MAX { None } else { Some(slice) },
        },
        nodes: HashMap::new(),
        by_continuation: HashMap::new(),
        next_id,
        now_millis,
        pending_releases: 0,
    };
    for _ in 0..count {
        let id = r.u32()?;
        let continuation_id = r.u32()?;
        let parent_raw = r.u32()?;
        let depth = r.u32()?;
        let status_tag = r.u8()?;
        let result = r.i32()?;
        let status = match status_tag {
            0 => GoalStatus::Running,
            1 => GoalStatus::Completed(result),
            2 => GoalStatus::Cancelled,
            _ => return Err(HostError::new("bad goal status")),
        };
        let charged_concurrent = r.u8()? != 0;
        let budget = r.u64()?;
        let deadline = r.u64()?;
        let nchildren = r.u32()? as usize;
        let mut children = Vec::with_capacity(nchildren);
        for _ in 0..nchildren {
            children.push(r.u32()?);
        }
        let cap_n = r.u32()?;
        let allowed_caps = if cap_n == u32::MAX {
            None
        } else {
            let mut caps = Vec::with_capacity(cap_n as usize);
            for _ in 0..cap_n {
                caps.push(CapHandle {
                    table_index: r.u32()?,
                    generation: r.u32()?,
                });
            }
            Some(caps)
        };
        goal.by_continuation.insert(continuation_id, id);
        goal.nodes.insert(
            id,
            GoalNode {
                id,
                continuation_id,
                parent_id: if parent_raw == u32::MAX {
                    None
                } else {
                    Some(parent_raw)
                },
                depth,
                children,
                status,
                allowed_caps,
                host_call_budget: if budget == u64::MAX {
                    None
                } else {
                    Some(budget)
                },
                charged_concurrent,
                deadline_millis: if deadline == 0 { None } else { Some(deadline) },
            },
        );
    }
    if r.pos != r.data.len() {
        return Err(HostError::new("trailing goal snapshot bytes"));
    }
    Ok(goal)
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn need(&self, n: usize) -> Result<(), HostError> {
        if self.pos + n > self.data.len() {
            Err(HostError::new("truncated goal snapshot"))
        } else {
            Ok(())
        }
    }

    fn u8(&mut self) -> Result<u8, HostError> {
        self.need(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32, HostError> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    fn u64(&mut self) -> Result<u64, HostError> {
        self.need(8)?;
        let v = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    fn i32(&mut self) -> Result<i32, HostError> {
        self.need(4)?;
        let v = i32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use crate::plugin::{Plugin, PluginContext, PluginResult};
    use crate::schema::{function_type, parse_package_schema};
    use tollana_core::{
        ExecOutcome, FunctionType, HostCall, Label, QuotaDimension, QuotaSlot, SuspendReason, Value,
    };

    const ECHO_SCHEMA: &[u8] = br#"{"name":"echo","version":"1.0.0","methods":[{"id":0,"name":"echo","params":["i32"],"results":["i32"],"capabilities":[]}]}"#;

    struct Echo;

    impl Plugin for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn schema(&self) -> &[u8] {
            ECHO_SCHEMA
        }
        fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
            let schema = parse_package_schema(ECHO_SCHEMA)?;
            schema
                .methods
                .iter()
                .map(|m| Ok((m.id, function_type(m)?)))
                .collect()
        }
        fn invoke(
            &mut self,
            _method_id: u32,
            _args: &[Value],
            _caps: &[CapHandle],
            _ctx: &mut dyn PluginContext,
        ) -> Result<PluginResult, HostError> {
            Ok(PluginResult::Pending(0))
        }
        fn snapshot_state(&self) -> Vec<u8> {
            Vec::new()
        }
        fn restore_state(&mut self, _bytes: &[u8]) -> Result<(), HostError> {
            Ok(())
        }
    }

    fn module(echo_id: u32, goal_id: u32) -> String {
        format!(
            r#"
(module
  (host.import Echo
    (pluginId {echo_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import goal.spawn
    (pluginId {goal_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import goal.join
    (pluginId {goal_id})
    (methodId 1)
    (param i32)
    (result i32))
  (host.import goal.cancel
    (pluginId {goal_id})
    (methodId 2)
    (param i32)
    (result unit))
  (func (export "child_a") (result i32)
    (host.invoke Echo (i32.const 1)))
  (func (export "child_b") (result i32)
    (host.invoke Echo (i32.const 2)))
  (func (export "main") (result i32)
    (local i32)
    (local i32)
    (local.set 0 (host.invoke goal.spawn (i32.const 0)))
    (local.set 1 (host.invoke goal.spawn (i32.const 1)))
    (i32.add
      (host.invoke goal.join (local.get 0))
      (host.invoke goal.join (local.get 1)))))
"#
        )
    }

    fn bind_echo_goal(goal: Goal) -> Host {
        let mut host = Host::new();
        host.register(Box::new(Echo)).unwrap();
        host.register(Box::new(goal)).unwrap();
        host.bind().unwrap();
        host
    }

    fn echo_pending(host: &Host) -> Vec<HostCall> {
        let echo_id = host.plugin_id("echo").unwrap();
        host.instance()
            .unwrap()
            .machine
            .pending_host_calls
            .iter()
            .filter(|c| c.plugin_id == echo_id)
            .cloned()
            .collect()
    }

    fn resume_echos(host: &mut Host) -> Result<ExecOutcome, HostError> {
        let calls = echo_pending(host);
        let mut outcome = ExecOutcome::Suspended {
            reason: SuspendReason::HostInvoke,
        };
        for call in calls {
            outcome = host.resume_continuation(call.continuation_identifier, call.arguments)?;
        }
        Ok(outcome)
    }

    #[test]
    fn parent_joins_two_children() {
        let mut host = bind_echo_goal(Goal::new());
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&module(echo_id, goal_id)).unwrap();
        match host.run("main", &[], 1000).unwrap() {
            ExecOutcome::Suspended {
                reason: SuspendReason::HostInvoke,
            } => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(echo_pending(&host).len(), 2);
        match resume_echos(&mut host).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(3, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallResumed"));
        assert!(!names.contains(&"GoalSpawned"));
    }

    #[test]
    fn snapshot_mid_wait_restores_tree() {
        let mut host = bind_echo_goal(Goal::new());
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&module(echo_id, goal_id)).unwrap();
        host.run("main", &[], 1000).unwrap();
        assert_eq!(echo_pending(&host).len(), 2);
        assert_eq!(host.instance().unwrap().machine.pending_host_calls.len(), 3);
        let bytes = host.snapshot().unwrap();

        let mut host2 = bind_echo_goal(Goal::new());
        host2.restore(&bytes).unwrap();
        assert_eq!(echo_pending(&host2).len(), 2);
        match resume_echos(&mut host2).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(3, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cancel_subtree_join_returns_zero() {
        let src_for = |echo_id, goal_id| {
            format!(
                r#"
(module
  (host.import Echo
    (pluginId {echo_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import goal.spawn
    (pluginId {goal_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import goal.join
    (pluginId {goal_id})
    (methodId 1)
    (param i32)
    (result i32))
  (host.import goal.cancel
    (pluginId {goal_id})
    (methodId 2)
    (param i32)
    (result unit))
  (func (export "child_a") (result i32)
    (host.invoke Echo (i32.const 1)))
  (func (export "child_b") (result i32)
    (host.invoke Echo (i32.const 2)))
  (func (export "main") (result i32)
    (local i32)
    (local i32)
    (local.set 0 (host.invoke goal.spawn (i32.const 0)))
    (local.set 1 (host.invoke goal.spawn (i32.const 1)))
    (drop (host.invoke goal.cancel (local.get 0)))
    (i32.add
      (host.invoke goal.join (local.get 0))
      (host.invoke goal.join (local.get 1)))))
"#
            )
        };
        let mut host = bind_echo_goal(Goal::new());
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&src_for(echo_id, goal_id)).unwrap();
        host.run("main", &[], 1000).unwrap();
        assert_eq!(echo_pending(&host).len(), 1);
        match resume_echos(&mut host).unwrap() {
            ExecOutcome::Completed { results } => {
                assert_eq!(results, vec![Value::i32(2, Label::Public)]);
            }
            other => panic!("{other:?}"),
        }
        assert!(host
            .instance()
            .unwrap()
            .journal
            .event_names()
            .contains(&"HostCallResumed"));
    }

    #[test]
    fn railguard_rejects_unbounded_depth() {
        let src_for = |echo_id, goal_id| {
            format!(
                r#"
(module
  (host.import Echo
    (pluginId {echo_id})
    (methodId 0)
    (param i32)
    (result i32))
  (host.import goal.spawn
    (pluginId {goal_id})
    (methodId 0)
    (param i32)
    (result i32))
  (func (export "leaf") (result i32)
    (host.invoke Echo (i32.const 9)))
  (func (export "child") (result i32)
    (host.invoke goal.spawn (i32.const 0)))
  (func (export "main") (result i32)
    (host.invoke goal.spawn (i32.const 1))))
"#
            )
        };
        let mut host = bind_echo_goal(Goal::new().with_railguards(Railguards {
            max_depth: 1,
            ..Railguards::default()
        }));
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&src_for(echo_id, goal_id)).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("max_depth"), "{err}");
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallFailed"));
    }

    #[test]
    fn concurrent_goals_quota_denies_third_spawn() {
        let mut host = bind_echo_goal(Goal::new());
        host.set_quotas(vec![QuotaSlot {
            dimension: QuotaDimension::ConcurrentGoals,
            remaining: 1,
        }]);
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&module(echo_id, goal_id)).unwrap();
        let err = host.run("main", &[], 1000).unwrap_err();
        assert!(err.message.contains("concurrent_goals_quota"), "{err}");
    }

    #[test]
    fn attenuate_children_journals() {
        let mut host = bind_echo_goal(Goal::new().with_railguards(Railguards {
            attenuate_children: true,
            ..Railguards::default()
        }));
        let echo_id = host.plugin_id("echo").unwrap();
        let goal_id = host.plugin_id("goal").unwrap();
        host.instantiate_text(&module(echo_id, goal_id)).unwrap();
        host.run("main", &[], 1000).unwrap();
        assert!(host
            .instance()
            .unwrap()
            .journal
            .event_names()
            .contains(&"HostCallSuspended"));
    }
}
