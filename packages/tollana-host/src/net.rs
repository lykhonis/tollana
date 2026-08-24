use crate::error::HostError;
use crate::plugin::{Plugin, PluginContext, PluginResult};
use crate::schema::{function_type, parse_package_schema, NET_SCHEMA_BYTES};
use std::collections::{BTreeMap, BTreeSet};
use tollana_core::{CapHandle, FunctionType, Label, QuotaDimension, Value};

pub const METHOD_FETCH: u32 = 0;
pub const METHOD_GET: u8 = 0;
pub const METHOD_POST: u8 = 1;
pub const METHOD_PUT: u8 = 2;
pub const METHOD_PATCH: u8 = 3;
pub const METHOD_DELETE: u8 = 4;
pub const METHOD_HEAD: u8 = 5;

const RECORD_VERSION: u8 = 1;

#[derive(Clone, Debug)]
struct Canned {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Clone, Debug)]
struct FetchRequest {
    method: u8,
    flags: u16,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Net {
    allow: BTreeSet<String>,
    replies: BTreeMap<String, Canned>,
}

impl Default for Net {
    fn default() -> Self {
        Self::double()
    }
}

impl Net {
    pub fn double() -> Self {
        Self {
            allow: BTreeSet::new(),
            replies: BTreeMap::new(),
        }
    }

    pub fn allow(&mut self, pattern: &str) {
        self.allow.insert(pattern.to_string());
    }

    pub fn reply(mut self, url: &str, status: u16, body: impl Into<Vec<u8>>) -> Self {
        self.replies.insert(
            url.to_string(),
            Canned {
                status,
                headers: Vec::new(),
                body: body.into(),
            },
        );
        self
    }

    fn allowed(&self, url: &str) -> bool {
        self.allow.iter().any(|pattern| url_matches(url, pattern))
    }
}

fn url_matches(url: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        url.starts_with(prefix)
    } else {
        url == pattern
    }
}

pub fn encode_request(
    method: u8,
    flags: u16,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut buf = vec![RECORD_VERSION, method];
    buf.extend_from_slice(&flags.to_le_bytes());
    push_bytes(&mut buf, url.as_bytes());
    buf.extend_from_slice(&(headers.len() as u32).to_le_bytes());
    for (name, value) in headers {
        push_bytes(&mut buf, name.as_bytes());
        push_bytes(&mut buf, value.as_bytes());
    }
    push_bytes(&mut buf, body);
    buf
}

pub fn decode_response(bytes: &[u8]) -> Result<FetchResponse, HostError> {
    let mut pos = 0;
    if bytes.first().copied() != Some(RECORD_VERSION) {
        return Err(HostError::new("invalid net response version"));
    }
    pos += 1;
    let status = read_u16(bytes, &mut pos)?;
    let _flags = read_u16(bytes, &mut pos)?;
    let n = read_u32(bytes, &mut pos)? as usize;
    let mut headers = Vec::with_capacity(n);
    for _ in 0..n {
        let name = read_utf8(bytes, &mut pos)?;
        let value = read_utf8(bytes, &mut pos)?;
        headers.push((name, value));
    }
    let body = read_len_bytes(bytes, &mut pos)?;
    if pos != bytes.len() {
        return Err(HostError::new("trailing net response bytes"));
    }
    Ok(FetchResponse {
        status,
        headers,
        body,
    })
}

fn encode_response(status: u16, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut buf = vec![RECORD_VERSION];
    buf.extend_from_slice(&status.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&(headers.len() as u32).to_le_bytes());
    for (name, value) in headers {
        push_bytes(&mut buf, name.as_bytes());
        push_bytes(&mut buf, value.as_bytes());
    }
    push_bytes(&mut buf, body);
    buf
}

fn decode_request(bytes: &[u8]) -> Result<FetchRequest, HostError> {
    let mut pos = 0;
    if bytes.first().copied() != Some(RECORD_VERSION) {
        return Err(HostError::new("invalid net request version"));
    }
    pos += 1;
    if pos >= bytes.len() {
        return Err(HostError::new("truncated net request"));
    }
    let method = bytes[pos];
    pos += 1;
    if method > METHOD_HEAD {
        return Err(HostError::new("unknown net method"));
    }
    let flags = read_u16(bytes, &mut pos)?;
    let url = read_utf8(bytes, &mut pos)?;
    let n = read_u32(bytes, &mut pos)? as usize;
    let mut headers = Vec::with_capacity(n);
    for _ in 0..n {
        let name = read_utf8(bytes, &mut pos)?;
        let value = read_utf8(bytes, &mut pos)?;
        headers.push((name, value));
    }
    let body = read_len_bytes(bytes, &mut pos)?;
    if pos != bytes.len() {
        return Err(HostError::new("trailing net request bytes"));
    }
    Ok(FetchRequest {
        method,
        flags,
        url,
        headers,
        body,
    })
}

fn push_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn read_u16(bytes: &[u8], pos: &mut usize) -> Result<u16, HostError> {
    if *pos + 2 > bytes.len() {
        return Err(HostError::new("truncated net record"));
    }
    let v = u16::from_le_bytes(bytes[*pos..*pos + 2].try_into().unwrap());
    *pos += 2;
    Ok(v)
}

fn read_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, HostError> {
    if *pos + 4 > bytes.len() {
        return Err(HostError::new("truncated net record"));
    }
    let v = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

fn read_len_bytes(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, HostError> {
    let len = read_u32(bytes, pos)? as usize;
    if *pos + len > bytes.len() {
        return Err(HostError::new("truncated net record"));
    }
    let out = bytes[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(out)
}

fn read_utf8(bytes: &[u8], pos: &mut usize) -> Result<String, HostError> {
    String::from_utf8(read_len_bytes(bytes, pos)?).map_err(|_| HostError::new("invalid net utf8"))
}

fn cap_arg(args: &[Value]) -> Result<CapHandle, HostError> {
    args.first()
        .and_then(|v| v.as_cap())
        .filter(|h| !h.is_null())
        .ok_or_else(|| HostError::new("net.fetch expects a live capability"))
}

fn i32_arg(args: &[Value], index: usize) -> Result<i32, HostError> {
    args.get(index)
        .and_then(|v| v.as_i32())
        .ok_or_else(|| HostError::new("net.fetch expects i32"))
}

impl Plugin for Net {
    fn name(&self) -> &str {
        "net"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> &[u8] {
        NET_SCHEMA_BYTES
    }

    fn methods(&self) -> Result<Vec<(u32, FunctionType)>, HostError> {
        let schema = parse_package_schema(NET_SCHEMA_BYTES)?;
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
        if method_id != METHOD_FETCH {
            return Err(HostError::new(format!("unknown net method {method_id}")));
        }
        let handle = cap_arg(args)?;
        if !caps.contains(&handle) {
            return Err(HostError::new("net_denied"));
        }
        let req_ptr = i32_arg(args, 1)?;
        let req_len = i32_arg(args, 2)?;
        let dst_ptr = i32_arg(args, 3)?;
        let dst_len = i32_arg(args, 4)?;
        let req_bytes = ctx.read_memory(req_ptr, req_len)?;
        let request = decode_request(&req_bytes)?;
        if !self.allowed(&request.url) {
            return Err(HostError::new("net_deny"));
        }
        let canned = self.replies.get(&request.url).cloned().unwrap_or(Canned {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        });
        let mut headers = canned.headers;
        for (name, value) in request.headers {
            if name.eq_ignore_ascii_case("content-type") {
                headers.push((name, value));
            }
        }
        if request.method == METHOD_POST && !request.body.is_empty() && canned.body.is_empty() {
            headers.push(("x-echo-method".into(), request.method.to_string()));
        }
        let body = if canned.body.is_empty() && !request.body.is_empty() {
            request.body.clone()
        } else {
            canned.body
        };
        let _ = request.flags;
        let response = encode_response(canned.status, &headers, &body);
        if response.len() > dst_len as u32 as usize {
            return Err(HostError::new("net_response_truncated"));
        }
        let charge = (req_bytes.len() as u64).saturating_add(response.len() as u64);
        if ctx.quota_remaining(QuotaDimension::IoBytes).is_some()
            && !ctx.consume_quota(QuotaDimension::IoBytes, charge)
        {
            return Err(HostError::new("io_bytes_quota"));
        }
        ctx.write_memory(dst_ptr, &response)?;
        Ok(PluginResult::Immediate(vec![Value::i32(
            response.len() as i32,
            Label::Public,
        )]))
    }

    fn snapshot_state(&self) -> Vec<u8> {
        let mut buf = vec![1];
        buf.extend_from_slice(&(self.allow.len() as u32).to_le_bytes());
        for pattern in &self.allow {
            push_bytes(&mut buf, pattern.as_bytes());
        }
        buf.extend_from_slice(&(self.replies.len() as u32).to_le_bytes());
        for (url, canned) in &self.replies {
            push_bytes(&mut buf, url.as_bytes());
            buf.extend_from_slice(&canned.status.to_le_bytes());
            buf.extend_from_slice(&(canned.headers.len() as u32).to_le_bytes());
            for (name, value) in &canned.headers {
                push_bytes(&mut buf, name.as_bytes());
                push_bytes(&mut buf, value.as_bytes());
            }
            push_bytes(&mut buf, &canned.body);
        }
        buf
    }

    fn restore_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if bytes.is_empty() || bytes[0] != 1 {
            return Err(HostError::new("invalid net snapshot blob"));
        }
        let mut pos = 1;
        let n_allow = read_u32(bytes, &mut pos)?;
        self.allow.clear();
        for _ in 0..n_allow {
            self.allow.insert(read_utf8(bytes, &mut pos)?);
        }
        let n_replies = read_u32(bytes, &mut pos)?;
        self.replies.clear();
        for _ in 0..n_replies {
            let url = read_utf8(bytes, &mut pos)?;
            let status = read_u16(bytes, &mut pos)?;
            let n_headers = read_u32(bytes, &mut pos)? as usize;
            let mut headers = Vec::with_capacity(n_headers);
            for _ in 0..n_headers {
                let name = read_utf8(bytes, &mut pos)?;
                let value = read_utf8(bytes, &mut pos)?;
                headers.push((name, value));
            }
            let body = read_len_bytes(bytes, &mut pos)?;
            self.replies.insert(
                url,
                Canned {
                    status,
                    headers,
                    body,
                },
            );
        }
        if pos != bytes.len() {
            return Err(HostError::new("trailing net snapshot bytes"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use tollana_core::{ExecOutcome, JournalEventKind, Label};

    const NET_CAP: CapHandle = CapHandle {
        table_index: 2,
        generation: 1,
    };
    const REQ_PTR: i32 = 0;
    const DST_PTR: i32 = 256;
    const DST_LEN: i32 = 512;

    fn fetch_module(plugin_id: u32, req_len: i32) -> String {
        format!(
            r#"
(module
  (memory (pages 1))
  (host.import net.fetch
    (pluginId {plugin_id})
    (methodId 0)
    (param Capability)
    (param i32)
    (param i32)
    (param i32)
    (param i32)
    (result i32))
  (func (export "main") (param Capability) (result i32)
    (host.invoke net.fetch
      (local.get 0)
      (i32.const {REQ_PTR})
      (i32.const {req_len})
      (i32.const {DST_PTR})
      (i32.const {DST_LEN}))))
"#
        )
    }

    fn cap() -> Value {
        Value::capability(NET_CAP, Label::Public)
    }

    fn wired(mut net: Net) -> (Host, u32) {
        net.allow("https://allowed.example/api");
        net.allow("https://allowed.example/items/*");
        let mut host = Host::new();
        host.register(Box::new(net)).unwrap();
        host.bind().unwrap();
        let id = host.plugin_id("net").unwrap();
        (host, id)
    }

    fn load_request(host: &mut Host, req: &[u8]) {
        host.write_linear_memory(REQ_PTR as usize, req).unwrap();
    }

    #[test]
    fn allow_listed_fetch_writes_response() {
        let (mut host, id) = wired(Net::double().reply("https://allowed.example/api", 200, b"ok"));
        let req = encode_request(METHOD_GET, 0, "https://allowed.example/api", &[], b"");
        host.instantiate_text(&fetch_module(id, req.len() as i32))
            .unwrap();
        host.grant_cap(NET_CAP, b"net".to_vec()).unwrap();
        load_request(&mut host, &req);
        let written = match host.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => results[0].as_i32().unwrap(),
            other => panic!("{other:?}"),
        };
        let raw = host
            .read_linear_memory(DST_PTR as usize, written as usize)
            .unwrap();
        let decoded = decode_response(&raw).unwrap();
        assert_eq!(decoded.status, 200);
        assert_eq!(decoded.body, b"ok");
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallResumed"));
    }

    #[test]
    fn post_with_headers_and_body_is_guest_owned() {
        let (mut host, id) = wired(Net::double());
        let url = "https://allowed.example/items/7";
        let req = encode_request(
            METHOD_POST,
            0,
            url,
            &[("content-type", "application/json")],
            b"{\"id\":7}",
        );
        host.instantiate_text(&fetch_module(id, req.len() as i32))
            .unwrap();
        host.grant_cap(NET_CAP, b"net".to_vec()).unwrap();
        load_request(&mut host, &req);
        let written = match host.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => results[0].as_i32().unwrap(),
            other => panic!("{other:?}"),
        };
        let raw = host
            .read_linear_memory(DST_PTR as usize, written as usize)
            .unwrap();
        let decoded = decode_response(&raw).unwrap();
        assert_eq!(decoded.status, 200);
        assert_eq!(decoded.body, b"{\"id\":7}");
        assert!(decoded
            .headers
            .iter()
            .any(|(n, v)| n == "content-type" && v == "application/json"));
        assert!(decoded
            .headers
            .iter()
            .any(|(n, v)| n == "x-echo-method" && v == "1"));
    }

    #[test]
    fn deny_is_journaled() {
        let (mut host, id) = wired(Net::double());
        let req = encode_request(METHOD_GET, 0, "https://evil.example/", &[], b"");
        host.instantiate_text(&fetch_module(id, req.len() as i32))
            .unwrap();
        host.grant_cap(NET_CAP, b"net".to_vec()).unwrap();
        load_request(&mut host, &req);
        let err = host.run("main", &[cap()], 1000).unwrap_err();
        assert!(err.message.contains("net_deny"), "{err}");
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
                assert_eq!(*method_id, METHOD_FETCH);
                assert!(message.contains("net_deny"));
            }
            other => panic!("{}", other.name()),
        }
        let names = host.instance().unwrap().journal.event_names();
        assert!(names.contains(&"HostCallSuspended"));
        assert!(names.contains(&"HostCallFailed"));
        assert!(!names.contains(&"HostCallResumed"));
    }

    #[test]
    fn snapshot_restores_allow_list() {
        let (mut host, id) =
            wired(Net::double().reply("https://allowed.example/api", 201, b"saved"));
        let req = encode_request(METHOD_GET, 0, "https://allowed.example/api", &[], b"");
        host.instantiate_text(&fetch_module(id, req.len() as i32))
            .unwrap();
        host.grant_cap(NET_CAP, b"net".to_vec()).unwrap();
        load_request(&mut host, &req);
        let written = match host.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => results[0].as_i32().unwrap(),
            other => panic!("{other:?}"),
        };
        let raw = host
            .read_linear_memory(DST_PTR as usize, written as usize)
            .unwrap();
        assert_eq!(decode_response(&raw).unwrap().status, 201);
        let bytes = host.snapshot().unwrap();
        let mut host2 = Host::new();
        host2.register(Box::new(Net::double())).unwrap();
        host2.restore(&bytes).unwrap();
        match host2.run("main", &[cap()], 1000).unwrap() {
            ExecOutcome::Completed { results } => {
                let n = results[0].as_i32().unwrap();
                let raw = host2
                    .read_linear_memory(DST_PTR as usize, n as usize)
                    .unwrap();
                let decoded = decode_response(&raw).unwrap();
                assert_eq!(decoded.status, 201);
                assert_eq!(decoded.body, b"saved");
            }
            other => panic!("{other:?}"),
        }
    }
}
