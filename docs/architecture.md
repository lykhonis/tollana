# RFC 0001 — Tollana Architecture

| Field | Value |
|-------|--------|
| **RFC** | 0001 |
| **Title** | Tollana Architecture |
| **Category** | Standards Track |
| **Status** | Proposed Standard |
| **Created** | 2026-08-19 |
| **Updates** | — |
| **Obsoletes** | — |
| **Relates** | [RFC 0002 — Tollana IR v0](bytecode.md) |
| **Repository** | [lykhonis/tollana](https://github.com/lykhonis/tollana) |

## Abstract

This document specifies the architecture of Tollana, a durable, portable agentic runtime. The name references the advanced civilization planet from *Stargate*.

The primary engine is an explicit stack-machine interpreter. Its native format is Tollana IR, specified in [RFC 0002](bytecode.md). External WASM runtimes (Wasmtime, Wasmer, custom, and others) are **not** part of the abstract machine. They MAY exist later only as optional, swappable language adapters. The core MAY be compiled to `wasm32` for deployment; that is a compilation target, not an execution dependency.

This RFC is normative. Implementation sequencing, repository layout, and pull-request order are not requirements of this document.

## Status of This Memo

This is a standards-track specification. Implementations MUST follow the requirements herein. Illustrative guest snippets, ASCII diagrams, and “well-known package” names are non-normative unless stated with RFC 2119 keywords.

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Conventions and Terminology](#2-conventions-and-terminology)
3. [Vision and Requirements](#3-vision-and-requirements)
4. [High-Level Architecture](#4-high-level-architecture)
5. [Core Execution Model](#5-core-execution-model)
6. [Plugin System and Identity](#6-plugin-system-and-identity)
7. [Guest Programming Model](#7-guest-programming-model)
8. [LLM-Generated Code Execution](#8-llm-generated-code-execution)
9. [Goals and Hierarchical Concurrency](#9-goals-and-hierarchical-concurrency)
10. [AI Gateway and Context](#10-ai-gateway-and-context)
11. [Snapshots, Time-Travel, and Replay](#11-snapshots-time-travel-and-replay)
12. [Quotas and Cost Control](#12-quotas-and-cost-control)
13. [Observability, Journal, and Developer Experience](#13-observability-journal-and-developer-experience)
14. [Security, Privacy, and Capability Model](#14-security-privacy-and-capability-model)
15. [Host Resources](#15-host-resources)
16. [Deployment and Portability](#16-deployment-and-portability)
17. [Out of Scope and Trade-offs](#17-out-of-scope-and-trade-offs)
18. [IANA Considerations](#18-iana-considerations)
19. [References](#19-references)
20. [Appendix A — Glossary](#appendix-a--glossary)

---

## 1. Introduction

Tollana is a runtime for long-running, hierarchical, observable AI agents. This RFC defines the host/guest/core split, the object-capability model, plugin identity, durability, quotas, and the relationship to Tollana IR.

The instruction set, encodings, validation, and `host.invoke` protocol are specified in [RFC 0002](bytecode.md). This RFC MUST NOT contradict RFC 0002; IR-level details in this document are informative summaries unless they use RFC 2119 keywords that RFC 0002 also states.

## 2. Conventions and Terminology

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

Canonical IR names (`host.invoke`, `pluginId`, `MachineState`, `Label`, …) follow RFC 0002. This RFC uses those names when referring to the instruction set. Architecture-level terms are defined in [Appendix A](#appendix-a--glossary).

Prose that describes future extensions (TEE, additional languages, dirty-page snapshots) is non-normative unless it uses RFC 2119 keywords.

---

## 3. Vision and Requirements

### What Tollana is

A runtime for long-running, hierarchical, observable AI agents that can be:

- Suspended and resumed with full state fidelity
- Migrated across machines and environments
- Inspected, replayed, and debugged with time-travel
- Metered for cost and billing
- Run from embedded devices and mobile phones (local models) through Docker and Kubernetes to WASM-hosted surfaces (e.g. TVs) — the core compiled *to* WASM, not depending on a WASM runtime to execute guests
- Executed under strong capability isolation and privacy controls

### Design pillars

| Pillar | Meaning |
|--------|---------|
| **Durability first** | The runtime owns the full abstract machine state; snapshots are exact |
| **Native-feeling concurrency** | Sequential `async/await` is the default; structured goal trees when needed |
| **Host-pluggable capabilities** | 100% codegen plugins; no hardcoded modules in the core; content-addressed identity |
| **Capability security & privacy** | Object-capability model, least privilege, sensitivity-aware data, local-first defaults |
| **Excellent DX** | Replay, time-travel, tracing, data-flow visualization, dwelling |
| **Extreme portability** | Same core; host supplies plugins, budgets, and backends |

### Success criteria

- A guest program can suspend mid-`await`, restore on another host, and continue correctly
- The AI can spawn focused sub-goals under host railguards
- Cost and token usage are attributable per goal and per run
- The same guest code runs against local on-device models and cloud gateways without changes
- Developers can scrub a run timeline, jump to any point, and inspect full state
- Guests have no ambient authority; every power is an explicit, attenuable capability
- Sensitive data (prompts, context, intermediate state) can be protected by policy, encryption, and local-only execution
- LLM-generated code runs as fully untrusted work in an isolated child machine, never as ambient parent authority
- A snapshot bound to plugin content hashes will not silently bind to a different plugin version on restore

---

## 4. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Host Process                         │
│                                                             │
│   Plugin resolution (registries, GitHub, local folders, …)  │
│   Content-hash identity → sorted local u32 IDs              │
│   Capability grants + privacy policy                        │
│                                                             │
│   Equal packages (codegen-only; no reserved IDs), e.g.:     │
│   ├── ai          AI Gateway                                │
│   ├── context     MCP Resources / knowledge                 │
│   ├── fs          Virtual filesystem                        │
│   ├── net         Network / HTTP                            │
│   ├── clock       Time + deterministic mode                 │
│   ├── random      Seedable entropy                          │
│   ├── goal        Hierarchical goal primitives              │
│   ├── code        Isolated untrusted code execution         │
│   ├── metrics     KPIs / evals                              │
│   └── …           Host-custom plugins                       │
└───────────────────────────┬─────────────────────────────────┘
                            │ register + grant capabilities
┌───────────────────────────▼─────────────────────────────────┐
│                     Tollana Core (Rust)                     │
│                                                             │
│  • Explicit stack-machine interpreter (Tollana IR)          │
│  • Delimited continuations + host-driven cooperative        │
│    scheduler (no OS threads in the core)                    │
│  • Generic plugin invoke (pluginId + methodId + payload)  │
│  • Multi-dimensional quotas                                 │
│  • Snapshot / restore engine                                │
│  • Journal (source of truth for audit & replay)             │
│  • Capability & sensitivity enforcement                     │
└───────────────────────────┬─────────────────────────────────┘
                            │ generated typed SDK
┌───────────────────────────▼─────────────────────────────────┐
│              Guest (TypeScript / Python / C / bytecode)     │
│                                                             │
│  await ai.chat(...)                                         │
│  await context.read("docs://…")                             │
│  await goal.run("research", async () => { … })              │
│  await code.run({ language, source, inputs, … })            │
│  log.info(...) / metrics.record(...)                        │
└─────────────────────────────────────────────────────────────┘
```

**Separation of concerns**

- **Core** knows nothing about AI providers, MCP, filesystems, or language semantics of plugins. It only suspends, resumes, meters, journals, snapshots, and enforces capabilities / quotas / sensitivity policy. Guest SDKs and the interpreter see only local `u32` plugin IDs.
- **Host** resolves plugin packages, computes content hashes, assigns local IDs, implements plugins, grants capabilities, and applies privacy policy.
- **Guest** writes natural async code against a generated typed surface and receives only the capabilities the host has explicitly granted.

---

## 5. Core Execution Model

### Explicit interpreter + Tollana IR

Primary execution is an **explicit stack-machine interpreter** that runs **Tollana IR / bytecode**. This is the core’s own execution format.

Reasons (own IR):

- Perfect ownership of stacks, frames, continuations, and linear memory
- Exact snapshots
- Deterministic fuel interruption
- Strong software sandbox
- Clean capability enforcement
- Straightforward journaling of instruction-level or host-call-level events

Language frontends (TypeScript, Python, minimal C, LLM-generated code, …) either **lower to Tollana IR** or **embed a snapshot-friendly runtime** whose state becomes part of (or an opaque blob inside) `MachineState`. External WASM guest execution, if ever added, is just another adapter. Optional hybrid JIT may be explored later; the interpreter remains the source of truth for state.

### Continuations and host-driven scheduling

Native concurrency uses **delimited continuations** plus a **host-driven cooperative scheduler**. There are **no OS threads inside the core**.

When guest code hits an awaitable plugin call (`host.invoke`):

1. The current continuation is captured
2. Control returns to the host scheduler
3. The host runs the plugin operation asynchronously
4. On completion, the continuation is resumed with the result

Two shapes of guest concurrency map onto this:

| Guest pattern | Core representation |
|---------------|---------------------|
| Sequential `async/await` | A single live continuation that suspends on each host call |
| True concurrency (`Promise.all`, `goal.all`, …) | Multiple sibling continuations. The language runtime / goal layer creates the join; the core only schedules ready continuations |

Sequential code never needs explicit `spawn` or channels. Those remain available for true parallelism and structured concurrency.

Quotas, fuel, cancellation, and capability attenuation are hierarchical (per goal subtree). Snapshots capture the entire set of live continuations plus pending host calls.

### Fuel and interruption

Every run carries remaining **fuel** (instruction budget) and other quota dimensions. The interpreter decrements fuel; at zero it suspends with `OutOfFuel` rather than running unbounded.

### Abstract machine state (conceptual)

```text
MachineState
├── value_stack
├── call_stacks / continuations   (all live fibers)
├── memory                        (linear memory or paged view)
├── globals
├── pc / active continuation ids
├── fuel + quota counters
├── capability table / live handles
├── plugin identity map           (local u32 → content hash + metadata)
└── scheduler queues
```

Value representation and the snapshot format MUST accommodate **capability handles** and **sensitivity labels** as specified in RFC 0002.

---

## 6. Plugin System and Identity

### Requirements

The core MUST NOT contain hardcoded plugins. Everything the guest imports (`ai`, `context`, `fs`, `code`, and others) MUST be produced by **codegen** from host-defined plugin schemas.

There MUST NOT be reserved plugin identifiers. All plugins — including well-maintained packages — are equal packages.

### Runtime contract

The core understands only the RFC 0002 host-call:

```text
host.invoke(pluginId, methodId, arguments, capabilities) -> result | suspend
```

- Dispatch MUST use numeric identifiers, never magic strings, on the hot path.
- Plugins may contribute **opaque serializable state** to snapshots.
- **Capability grants are per instance and per goal subtree**: a guest only sees and can exercise the capabilities the host explicitly handed it.
- Capabilities are first-class values (see §14). They can be attenuated, passed, and revoked.

### Two-level identity (decentralized)

| Layer | Purpose | Form |
|-------|---------|------|
| **Global identity** | Durable, content-addressed, decentralized | `SHA-256` of the [v0 canonical identity bytes](#canonical-identity-bytes-v0) |
| **Local dense ID** | Hot-path numeric handle | `u32` assigned per instance |

Guest SDKs and the interpreter see **only** the local `u32`s. Durable identity lives in the host and in snapshots. Identity is the content hash, never a well-known name, never a reserved `pluginId`, and never a registry or GitHub coordinate.

### Canonical identity bytes (v0)

The preimage of a plugin’s identity hash is a single little-endian byte string. Field order is fixed. Implementations MUST encode this layout exactly; they MUST NOT insert padding, domain tags other than `TLID`, or a trailing checksum.

```text
magic:                 54 4C 49 44     ; ASCII "TLID" (domain separator, not a file magic)
identityVersion:       u16 = 1
nameLength:            u32
name:                  nameLength bytes, UTF-8
versionLength:         u32
version:               versionLength bytes, UTF-8
schemaLength:          u32
schema:                schemaLength bytes, opaque schema document
metadataLength:        u32
metadata:              metadataLength bytes, opaque (MAY be empty)
implementationPresent: u8              ; 0 = absent, 1 = present
implementationDigest:  32 bytes        ; only if implementationPresent = 1
```

`identityVersion` is part of the hashed preimage so a future layout can bump the field and produce a different hash without a second hasher. Encoders MUST write `identityVersion = 1`. Restore matches `identityHash` only; it MUST NOT parse these canonical bytes out of a snapshot.

- `name` and `version` MUST be non-empty UTF-8 and MUST NOT contain U+0000.
- `schema` is the plugin schema document bytes as supplied by the host. MUST NOT be replaced by a nested digest unless those digest bytes *are* the schema document the host chose to hash. Empty schema is legal.
- `metadata` is opaque and MUST be present as a length-prefixed field even when empty (`metadataLength = 0`).
- `implementationPresent` other than `0` or `1` is not a valid canonical encoding. When `0`, `implementationDigest` MUST be omitted. When `1`, `implementationDigest` MUST be the 32-byte SHA-256 of the implementation artifact the host is binding (the core does not compute this digest).
- Local folders and remote packages MUST produce this same preimage from the same `(name, version, schema, metadata, optional implementation digest)`.

**Identity hash:** `identityHash = SHA-256(canonical bytes)` (exactly 32 bytes). A function that hashes **already-canonical** bytes MUST be SHA-256 of those bytes with no extra prefix.

Omitting the implementation digest versus including one MUST yield different hashes. Changing `name`, `version`, `schema`, or `metadata` MUST yield a different hash.

### Assignment rules

1. Host resolves plugins (registries, GitHub, local folders, …).
2. Encodes canonical identity bytes and computes `identityHash` for each — this is the identity.
3. Default: sort by `identityHash` as unsigned 32-byte strings (memcmp / lexicographic byte order) and assign local IDs `0..N` in that order. Duplicate `identityHash` values MUST be rejected. A host MAY choose another stable total order if it writes those same local IDs into the guest module; interoperable tests MUST use sort-by-hash.
4. Guest SDKs and the interpreter see only those local `u32`s. The interpreter MUST NOT re-number `pluginId`s at instantiate (RFC 0002).
5. Snapshot stores the full mapping `{local_id, identity_hash, name, version}`.
6. On restore the host MUST supply matching content hashes and re-apply the **same** local identifiers.

**Local folders** and remote packages MUST use the **exact same hashing rules**.

### Golden vectors (v0)

All hashes are SHA-256 of the canonical encoding above, lowercase hex.

**Vector A** — `name = "echo"`, `version = "1.0.0"`, `schema = "(schema echo v1)"` (ASCII), empty metadata, implementation digest absent.

Canonical bytes (48 bytes):

```text
54 4c 49 44 01 00 04 00 00 00 65 63 68 6f 05 00
00 00 31 2e 30 2e 30 10 00 00 00 28 73 63 68 65
6d 61 20 65 63 68 6f 20 76 31 29 00 00 00 00 00
```

`identityHash` =

`b53818f082a602686525d386618246569a4f74a4997aa3dbe5006f5644ab5ba3`

**Vector B** — same as A except `version = "2.0.0"`.

`identityHash` =

`db97a544bfdef8e2fd10e80b152b7418568c3f856b72bff0701de5d8b335cb2d`

**Vector C** — same as A except `implementationPresent = 1` and `implementationDigest` is 32 bytes `0x11`.

`identityHash` =

`d93ad774747d2adb651866d0d24ef29783f0dcd0f431cda761da1c03473f67d0`

**Vector D** — `name = "clock"`, `version = "1.0.0"`, `schema = "(schema clock v1)"` (ASCII), empty metadata, implementation digest absent.

`identityHash` =

`2a1ca0a188fe61311c1dd8d9f73f2685d9ae8e6199db222da78977c3d3526dfa`

D’s hash is lexicographically less than A’s. Sort-by-hash of `{A, D}` MUST assign local id `0` to **clock** (D) and local id `1` to **echo** (A).

### Upgrades

A new version is a different content hash and therefore a **different identity**. A snapshot taken against v1 MUST NOT silently bind to v2. Restore MUST fail fast on hash mismatch. Same package name with a different hash is a different identity.

### Versioning (compatibility later)

v0 encodings are the current development formats. They are versioned so a later freeze can keep them without dual decode paths:

- TIRS `formatVersion`, container `containerVersion`, and identity `identityVersion` MUST be written as specified in this RFC (and RFC 0002). Decoders MUST reject unknown versions (fail closed). MUST NOT fall back to an older layout.
- Restore MUST NOT bind a different `identityHash`.
- Until a version is frozen as a published compatibility promise, a later RFC amendment MAY bump these fields. After freeze, a layout change MUST bump the corresponding version. There is no implicit `[0u8;32]` plugin identity and no reserved `pluginId`.

### Flow

1. Host author defines plugin schemas (methods, types, optional state shape, capability requirements).
2. Codegen emits:
   - Host-side registration and dispatch
   - Typed TypeScript guest SDK
   - Typed Python guest SDK
   - (Later) C / other language bindings
3. At instance creation the host resolves packages, hashes them, assigns local IDs, registers implementations, **and grants specific capabilities**.
4. Guest code imports only the generated modules and receives only the capabilities that were granted.

### Well-known packages

`ai`, `context`, `fs`, `net`, `clock`, `random`, `goal`, `code`, `metrics`, and `log` are **well-maintained codegen inputs**, not special-cased core modules and not reserved numeric IDs. Hosts may omit, replace, or extend them. Identity is always the content hash, never a well-known name.

### Why codegen-only

- Core stays stable and lean
- Hosts customize freely without forking the VM
- Guest APIs stay typed (no stringly `hostCall("ai", "chat", …)`)
- Different deployments expose different capability sets from the same guest language surface pattern
- Decentralized identity (content hash) plus dense local IDs keeps the hot path numeric without baking a plugin registry into the core

---

## 7. Guest Programming Model

### Languages (v1 and near-term)

- **TypeScript** (via controllable embedded runtime, e.g. QuickJS-style)
- **Python** (controlled, snapshot-friendly path)

A **minimal C or pure Tollana bytecode** frontend is a strong early target. Because the interpreter mediates every memory access and every host call, a C / bytecode guest runs inside a pure software sandbox with no ambient authority. This is the best security boundary the system can offer and a good vehicle for validating the machine before full high-level adapters are complete.

Language adapters either:

- **Lower** source to Tollana IR, or
- **Embed** a snapshot-friendly runtime whose heap/state becomes part of (or an opaque blob inside) `MachineState`

Not every guest language is a v1 goal. External WASM guest execution, if ever added, is just another adapter.

### Default style: natural `async/await`

```ts
import { ai, context } from "./generated";

export async function main(input: { query: string }) {
  const spec = await context.read("docs://api/spec.md");

  const reply = await ai.chat({
    messages: [
      { role: "system", content: "You are a helpful assistant." },
      { role: "user", content: input.query },
    ],
    context: [spec],
  });

  return reply.content;
}
```

No forced `spawn` / channels for linear control flow. Under the hood this is one live continuation that suspends on each `host.invoke`.

### Explicit concurrency (when needed)

```ts
const [a, b] = await Promise.all([
  context.read("docs://a"),
  context.read("docs://b"),
]);

// or structured goals — see §9
```

`Promise.all` / `goal.all` become sibling continuations. The language runtime or goal layer owns the join; the core only schedules ready continuations.

### SDK shape

- Public API: typed methods only
- Internal bridge: numeric `pluginId` / `methodId` (generated, private; identifiers are instance-local)
- Host never requires guests to pass provider names, API keys, or transport details
- Capability handles are passed explicitly or held in the instance context; guests cannot forge them

---

## 8. LLM-Generated Code Execution

LLM-generated code is **fully untrusted**. It is not a special case of the core: it uses the same numeric `invoke` + continuation model as every other plugin.

### Isolation

It always runs in a **fresh, isolated child `MachineState`** with:

- Tight fuel / memory / wall-time quotas
- Capability set = intersection of explicitly passed capabilities and a host “code-exec” allow-list (**normally empty**)
- No shared memory with the parent except via explicit `inputs`

### Surface

Exposed as a `code` plugin (`code.run`) and optionally as `goal.run_code`. The `code` plugin is an equal package: no reserved ID, content-hashed like everything else.

Sketch of the method:

```text
code.run({
  language: "python" | "javascript" | "bytecode",
  source: string,
  inputs?: map,
  timeout_ms?, fuel?, memory_limit?,
  capabilities?: list<capability_handle>,
  sensitivity?: label
}) → { status, result?, error?, stdout?, stderr?, metrics }
```

### Journal

The journal records source (or its hash), inputs, result, metrics, and any capability / policy denials.

---

## 9. Goals and Hierarchical Concurrency

### Goal trees

Agents decompose work into a **tree of goals**:

- Parent goals spawn children
- Children may spawn further sub-goals
- Parents await, cancel, or aggregate children
- Each node can carry focused context, quotas, **and a restricted capability set**

### Available to developers and to the AI

- Developer API: `goal.run`, `goal.all`, `goal.race`, cancellation scopes, optional `goal.run_code`
- **Tool surface for the model**: the AI may call a `spawn_goal` (or equivalent) tool so it can decide when to fan out

### Railguards (host-enforced)

| Control | Purpose |
|---------|---------|
| Max tree depth | Prevent unbounded decomposition |
| Max concurrent goals | Bound parallelism and cost |
| Max children per parent | Control fan-out |
| Per-subtree quotas | Token / fuel budgets per branch |
| Capability restrictions | Reduced or attenuated permissions in children |
| Timeouts | Kill runaway branches |
| Approval gates | Human-in-the-loop beyond depth N |

Violations reject spawn, downgrade to sequential execution, escalate, or log — policy is host-defined.

### Runtime representation

```text
Root continuation
├── goal: research
│   ├── ai.chat
│   └── context.read
├── goal: analyze
└── goal: synthesize
    └── goal: deep-dive
```

Snapshots capture the entire live tree (all live continuations + pending host calls). Cancellation is hierarchical. Capability attenuation is hierarchical as well.

---

## 10. AI Gateway and Context

### AI Gateway

All model traffic goes through a host-provided **AI Gateway** plugin.

Guest sees a stable high-level API, for example:

- `ai.chat`
- `ai.generate`
- `ai.stream`
- `ai.embed`

Host may implement the gateway with:

- Direct provider SDKs
- Corporate proxies
- Multi-provider routers and fallbacks
- **Local on-device models** (llama.cpp, MLX, Core ML, ONNX, ExecuTorch, …) — preferred default for privacy
- MCP tool aggregation

Guest code does not change when the backend changes.

Privacy posture: the host (and optionally sensitivity labels on data) decides whether a call may leave the device. Local-first is the encouraged default.

### Context (MCP Resources)

MCP **Resources** are application-controlled, URI-addressable, mostly read-only data (docs, schemas, configs, records).

Guest API (conceptual):

```ts
await context.list({ prefix: "docs://" });
await context.read("docs://api/spec.md");
for await (const update of context.watch("config://app/settings")) { … }
```

Notes:

- Distinct from **Tools** (model-controlled actions)
- Host maps one or more MCP servers (and non-MCP sources) into the URI space the guest sees
- Resources may be passed explicitly into `ai.chat` / `ai.generate`, or auto-attached by host policy
- Resources can carry sensitivity labels that constrain how they may be used

---

## 11. Snapshots, Time-Travel, and Replay

### Snapshot contents

```text
Snapshot
├── Header (magic, version, flags, instance id, checksum / AEAD tag)
├── Core machine state (stacks, continuations, memory, globals, fuel, quotas, live capabilities)
├── Plugin identity map ({local_id, identity_hash, name, version} for every registered plugin)
├── Plugin state entries (pluginId, opaque blob)
├── Pending / in-flight host calls
└── Metadata (journal cursor, optional debug refs)
```

### Format

- **Primary (v0):** the **snapshot container** specified below. Hand-written little-endian layout. MUST NOT require rkyv, bincode, or a second encoding of core machine state.
- Core machine state, plugin identity map, and pending host calls MUST be the RFC 0002 **TIRS** payload inside the container. The container MUST NOT fork a second `CoreSnapshot` encoding.
- **Integrity & confidentiality:** header supports AEAD encryption under a host-managed key; plain checksum mode remains available for non-sensitive or development use.
- Memory: full dump first (inside TIRS); dirty-page / incremental snapshots later.
- Optional compression (e.g. zstd) is out of v0.
- Treated as an opaque blob by storage backends.
- Optional binding to attestation identity or host so a snapshot cannot be restored on an unauthorized machine (out of v0).

### v0 snapshot container (normative)

Magic `54 4C 4E 41` (`TLNA`). Distinct from RFC 0002 TIRS (`54 49 52 53`) and module magic `TIR\0`.

Little-endian. Field name **`containerVersion`** (MUST NOT be called `formatVersion`; that name is TIRS).

```text
magic:                    54 4C 4E 41
containerVersion:         u16 = 1
flags:                    u16
                          bit 0 = 0 → checksum mode
                          bit 0 = 1 → AEAD mode
                          bits 1–15 MUST be 0
instanceId:               16 bytes   ; host-assigned; tests MAY use zeros
integrity:                32 bytes   ; checksum mode: SHA-256 of body
                                     ; AEAD mode: 16-byte AES-256-GCM tag, then 16 zero bytes
nonce:                    12 bytes   ; AEAD mode: GCM nonce; checksum mode: 12 zero bytes
bodyLength:               u32
body:                     that many bytes
```

**Body** (checksum mode: plaintext; AEAD mode: AES-256-GCM ciphertext of the same plaintext layout):

```text
tirsLength:               u32
tirsBytes:                RFC 0002 TIRS
pluginStateCount:         u32
for each:
  pluginId:               u32
  blobLength:             u32
  blob:                   opaque plugin state
journalCursor:            u64
```

Plugin-state rows are **`(pluginId, blob)` only**. MUST NOT store content hashes in this table; hashes live in TIRS. Integrity (SHA-256 of body in checksum mode; GCM over body with AAD defined below in AEAD mode) MUST cover TIRS + plugin-state table + journal cursor. Decode MUST fail closed on bad magic, `containerVersion ≠ 1`, nonzero reserved flag bits, truncated body, or integrity failure.

**Checksum mode:** `integrity[0..32) = SHA-256(body)`. `nonce` MUST be 12 zero bytes.

**AEAD mode:** AES-256-GCM, 256-bit host-supplied key. AAD is `magic || containerVersion || flags || instanceId` (26 bytes). Ciphertext is the body. Tag is 16 bytes in `integrity[0..16)`; `integrity[16..32)` MUST be zero. `nonce` is 12 random or host-supplied bytes, unique per snapshot under that key.

Unknown `containerVersion` MUST reject. A valid TIRS blob presented as a container MUST reject (wrong magic).

### Restore

1. Validate header, version, checksum / AEAD tag
2. Decrypt if necessary
3. Rebuild core machine (including capability table)
4. Host supplies plugins whose **content hashes match** the snapshot’s identity map, and re-applies the **same local IDs**
5. Host feeds opaque plugin state into those matching plugins
6. Re-bind or fail pending calls
7. Resume scheduler

Plugin identity-hash mismatch, capability mismatch, or missing plugin MUST fail fast. There is no silent upgrade path: v2 of a package is a different identity than v1.

### Time-travel

Snapshots are random-access checkpoints. Combined with the journal, a debugger can:

- Load any snapshot
- Replay forward to a specific event
- Inspect full state at that point
- Optionally fork and try a different path

### Replay modes

| Mode | Behavior |
|------|----------|
| **Strict** | Bit-identical re-execution using recorded nondeterminism |
| **Semantic** | Same structure and inputs; allow model/version variation (useful for evals) |

Nondeterministic inputs (model outputs, network, random, clock) are controlled or recorded in the journal for strict replay.

---

## 12. Quotas and Cost Control

### Multi-dimensional quotas

Examples:

- Instruction fuel
- Memory
- Token usage (in/out, per model)
- I/O bytes and request counts
- Host-call counts
- Concurrent goals
- Wall time (especially for isolated `code.run` children)

Quotas may be **hierarchical** (subtree inherits or receives a slice of parent budget). Isolated child machines for untrusted code get their own tight caps independent of leftover parent budget except where the host explicitly slices.

### Integration

- Enforced by the core scheduler / interpreter and by plugin metering
- Remaining quotas are part of snapshots
- Journal records consumption for billing and reports
- Host binds quotas to billing accounts or tenants at instance creation

### Billing hook

Execution is intentionally stoppable and attributable so hosts can sell “run until budget” or “pay per goal tree” without trusting guest code.

---

## 13. Observability, Journal, and Developer Experience

### Journal as source of truth

The journal is a dense, ordered event log. Derived views (metrics, traces, UI) build on it.

Automatically recorded (non-exhaustive):

- Goal lifecycle (start, end, status, parent/child)
- AI calls (model, tokens, latency, cost estimates)
- Isolated code execution (source or hash, inputs, result, metrics, denials)
- Context reads / watches
- FS and network operations (when those plugins are enabled)
- Quota consumption
- Capability grants, attenuations, and denials
- Plugin identity (hash, name, version) at instance creation and restore
- Snapshots, restores, errors, cancellations

**Correlation IDs:** `run_id` → `goal_id` → `parent_goal_id` → `ai_call_id` → …

Sensitivity labels travel into the journal; host policy redacts or restricts export of confidential/secret fields.

### Journal v0 envelope (in-process)

v0 is an **in-process** append-only log, not a durable journal file (that is a later slice). The container `journalCursor` reserved in §11 is the sink’s **next sequence number** after snapshot events have been appended. It is **advisory**: restore MUST NOT reconstitute events from the cursor. Restore **without** a log is legal.

Each event MUST have:

```text
sequence:     u64              ; 0-based, strictly increasing by 1 per append in a sink
run_id:       16 bytes         ; host-assigned; tests MAY use zeros; constant for a run
sensitivity:  Label            ; join of payload labels, or Public if none
event_type:   canonical name   ; UTF-8; see table
body:         event-specific fields (in-process; not a file layout)
```

**IR-level names** (RFC 0002 Observability) MUST be used when the interpreter emits those events:

| `event_type` | When | Body (minimum) |
|--------------|------|----------------|
| `InstructionStepped` | Each instruction (optional; **default off**) | `functionIndex`, `instructionIndex`, opcode name |
| `FuelSuspended` | Guest suspends `OutOfFuel` | `remainingFuel` (0), `ProgramCounter` |
| `FuelResumed` | Host `AddFuel` | `remainingFuel` after add (guest runs only on the following `Continue`) |
| `HostCallSuspended` | `host.invoke` suspend | `pluginId`, `methodId`, arity; argument types/labels; payloads redacted per policy |
| `HostCallResumed` | successful `Resume` | result types/labels; payloads redacted per policy |
| `Trapped` | guest trap | `TrapKind`, `ProgramCounter` |
| `Completed` | guest returns from entry | result types/labels; payloads redacted per policy |
| `SnapshotCoreTaken` | `SnapshotCore` (including when the container path calls it) | module length, fuel, memory length, continuation count |
| `SnapshotCoreRestored` | `RestoreCore` (including when the container path calls it) | same |
| `InvalidCapabilityUse` | invoke with a non-live capability | `tableIndex`, `generation` (no host secrets) |
| `InstanceCreated` | successful instantiate | plugin identity map (`pluginId`, `identityHash`, name, version) |

**Architecture-level names** (container path, in addition to the `SnapshotCore*` events):

| `event_type` | When | Body (minimum) |
|--------------|------|----------------|
| `SnapshotTaken` | byte `snapshot` / `snapshot_aead` after TIRS is encoded | `journalCursor` written into the container |
| `SnapshotRestored` | byte `restore` after `RestoreCore` | `journalCursor` read from the container |

A byte-level `snapshot` / `restore` MUST emit `SnapshotCoreTaken` / `SnapshotCoreRestored` because it calls `SnapshotCore` / `RestoreCore`.

**Default sink:** MUST redact `Confidential` and `Secret` **payloads** (types and labels remain). MUST NOT emit `InstructionStepped` unless the host enables it. Appending journal events MUST NOT decrement IR fuel.

**Same-process restore:** attaching the **same** sink MUST continue `sequence` (no reset, no duplicate prior events). New restore events MAY append with the next sequences. A different or empty sink starts at `sequence = 0`.

### Structured logging

Guest-facing `log` API with levels (`trace` … `fatal`), structured fields, and automatic enrichment from run/goal context. Host configures level, sampling, **redaction**, and sinks.

### Metrics / KPIs / evals

- Automatic: tokens, latency, goal counts, error rates, cost
- Explicit: `metrics.record`, counters, histograms, custom business KPIs
- Evaluation scores (rubrics, LLM-as-judge) stored with the run
- Reports generated from journal + metrics (HTML, Markdown, JSON, etc.)

### Tracing

Hierarchical spans for runs, goals, AI calls, isolated code children, and significant plugin ops. OpenTelemetry export supported; internal model retains goal-tree semantics.

### DX surfaces

| Capability | Description |
|------------|-------------|
| **Replay** | Strict or semantic re-execution |
| **Time travel** | Jump to snapshot + event |
| **Dwelling** | Deep inspection of stacks, memory, prompts, binaries, quotas, capabilities |
| **Visualization** | Goal tree, AI call graph, data lineage, timeline/flame, quota charts |
| **CLI** | `inspect`, `replay`, `timeline` (names illustrative) |
| **Web inspector** | Scrubber + tree + detail panes |

Redaction and access control are first-class for sensitive prompts, tool payloads, generated source, and capability metadata.

---

## 14. Security, Privacy, and Capability Model

Security and privacy are first-class design constraints, not afterthoughts. The interpreter-owned abstract machine is the primary enforcement point.

### Object-capability foundation

- Guests have **no ambient authority**.
- Every power (FS subtree, network allow-list, AI model access, code-exec, etc.) is represented as an **unforgeable capability token** granted by the host at instance creation or attenuated from a parent goal.
- Capabilities are first-class values in the machine. They can be passed to children, attenuated (reduced rights), or dropped. They cannot be forged or synthesized by guest code.
- Plugin invoke carries the relevant capability handles; the core and the host implementation both validate them.

This model maps cleanly onto the numeric plugin invoke path and onto hierarchical goals.

### Memory isolation & bounds checking

Because the core owns linear memory and the interpreter executes every instruction:

- Every load and store is bounds-checked.
- Guests (including a C guest) cannot escape the software sandbox or inspect other instances.
- Separate linear memories per instance (or per high-isolation goal subtree) are supported.
- Isolated `code.run` children get a **fresh `MachineState`** and do not share memory with the parent except via explicit `inputs`.
- Optional ephemeral memory regions can be wiped on snapshot boundaries or goal completion (useful for secrets).

A minimal C or bytecode guest is therefore one of the *safest* execution environments the system can offer: there are no raw syscalls and no ambient OS authority.

### Sensitivity / provenance labels

Values may carry a compact sensitivity tag: `public | internal | confidential | secret`.

- Context resources and AI inputs/outputs inherit or receive labels.
- Host policy (or a dedicated policy plugin) can enforce rules such as “confidential data may never be sent to an external model provider” or “this journal field must be redacted”.
- Labels travel with data into snapshots and the journal.
- The mechanism is intentionally lightweight; a full information-flow type system is not required on day one.

### Snapshot protection

- Snapshots support AEAD encryption under host-managed keys.
- Integrity is always protected (checksum or AEAD tag).
- Optional binding to a host or attestation identity prevents restoration on unauthorized machines.
- Language-runtime heaps (QuickJS, etc.) are treated as opaque blobs that participate in the same protection regime.

### Local-first and confidential execution

- The AI Gateway encourages local / on-device models as the default when available.
- Host policy can force “data never leaves this device”.
- Longer-term: the Tollana core itself can run inside a TEE (SEV, TDX, Nitro Enclaves, etc.) so that even the host operator cannot inspect live agent state. The TEE path is reserved; it is not required by this RFC.

### Least privilege by construction

- Instance creation starts with the minimal set of capabilities the host chooses to grant.
- Goal trees can further attenuate capabilities for children.
- Isolated generated-code children start from the intersection of passed capabilities and a host allow-list that is normally empty.
- Quotas and railguards act as additional isolation and DoS-prevention layers.
- The journal records capability-related events (grants, denials, attenuations) for audit.

### Design rules that keep the core pure

- The core never interprets the *meaning* of a capability beyond validation and table lookup; policy lives in the host and in the labels.
- All side effects and nondeterminism continue to flow through the single `host.invoke` channel so they remain journaled and snapshot-aware.
- Capability and sensitivity support is designed into the value representation and snapshot format early, even if full attenuation and label propagation land after the initial MVP.
- Plugin identity is content-hashed; the core never special-cases a plugin by well-known numeric ID.

---

## 15. Host Resources

Essential plugins beyond AI/context/goal/metrics/code:

| Plugin | Role |
|--------|------|
| **fs** | Virtual FS, capability-scoped preopens, multiple backends (memory, restricted disk, object store) |
| **net / http** | Async outbound HTTP(S), optional sockets; allow-lists, proxies, metering |
| **clock** | Wall + monotonic time, timers; **deterministic / virtual clock** for replay |
| **random** | Secure random + seedable PRNG for deterministic mode |

Also useful: stdio capture, config/secrets as capabilities, guest `log`.

Usually restricted or omitted: subprocesses, raw threads (continuations replace them), unconstrained env. Isolated `code.run` does not imply a subprocess; it is another `MachineState` scheduled by the core.

All resource access is capability-based, awaitable, journaled, and snapshot-aware.

---

## 16. Deployment and Portability

### Scale spectrum

| Environment | Host supplies |
|-------------|---------------|
| Embedded / mobile | Local AI Gateway, tiny FS, tight quotas, minimal plugins, strong local-only policy |
| Laptop / single host | Full plugin set, local or remote models |
| Docker | Containerized host + VM |
| Kubernetes | Cloud gateway, durable snapshot/journal storage, multi-tenant isolation, capability isolation between tenants |
| WASM-hosted (TV, edge, browser) | Outer WASM host implements plugins; Tollana core may itself be compiled to `wasm32`. This is a **compilation target**, not an execution dependency of the abstract machine. |
| Confidential computing | Core inside TEE; encrypted snapshots; attested configuration |

### Portability rules

- Core remains lean; heavy exporters and optional subsystems are feature-flagged or omitted via codegen
- Guest modules are portable; hosts change, guests do not
- Snapshots and journals scale down (ring buffer / in-memory) and up (object storage)
- Strong quotas and capabilities make the same code safe on a phone and on a fat cluster node
- Plugin packages are content-addressed; a restore requires the same hashes, wherever they were resolved from

### Embedding

Clean FFI / language bindings so the VM embeds into mobile apps, firmware hosts, and larger services.

---

## 17. Out of Scope and Trade-offs

### Out of scope

This specification does **not** require, and implementations MUST NOT treat as architectural requirements:

- Competing with Wasmtime/Wasmer on raw peak throughput for non-durable workloads
- Hard-wiring any external WASM runtime into the core abstract machine
- Supporting every guest language
- Silent upgrades of plugins under an existing snapshot
- Hardcoding cloud provider SDKs into the core
- Reserved / well-known plugin numeric identifiers
- Bit-identical replay when hosts use semantic replay or live external systems without recording
- A full information-flow type system or formal verification of every guest program

The core MUST NOT hard-wire an external WASM runtime. Restore MUST NOT silently bind a snapshot to a different plugin identity. Plugin numeric identifiers MUST NOT be reserved.

### Accepted trade-offs (informational)

| Choice | Cost | Benefit |
|--------|------|---------|
| Interpreter-first | Lower peak speed | Perfect snapshots, simple interruption, strong sandbox |
| Own IR instead of a WASM engine | Extra frontend / adapter work | Exact ownership of stacks, frames, continuations, memory |
| Codegen-only plugins | Extra toolchain step | No core special cases; typed guests |
| Content-hashed plugin identity | Restore requires exact hashes | Decentralized identity; no silent upgrades |
| Embedded language runtimes for TS/Python | Footprint and complexity | Snapshottable high-level DX |
| Isolated child machine for generated code | Extra instances | Untrusted LLM code cannot inherit parent authority |
| Rich journal | Storage | Replay, billing, DX, audit |
| Capability & label machinery | Design & runtime cost | Least privilege, privacy controls, safe multi-tenancy |

---

## 18. IANA Considerations

This document has no IANA actions.

## 19. References

### Normative

- [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119) — Key words for use in RFCs to Indicate Requirement Levels
- [RFC 0002](bytecode.md) — Tollana Intermediate Representation Version 0

### Informative

- WebAssembly Core Specification — prior art for structured control; not an execution dependency

---

## Appendix A — Glossary

| Term | Meaning |
|------|---------|
| **Capability** | Unforgeable, attenuable token that grants a specific right. Guests MUST NOT create them. |
| **Continuation** | Captured guest execution state that can be suspended and resumed |
| **Goal** | Structured concurrent unit of work in a parent/child tree |
| **host.invoke** | The only host-call instruction (RFC 0002); suspends the current continuation and yields a pending plugin invoke |
| **Journal** | Ordered event log; source of truth for audit and replay |
| **pluginId** | Dense local numeric handle assigned per instance after hashing and sorting (RFC 0002) |
| **Plugin** | Host-provided capability exposed to guests via codegen; equal package, no reserved identifier |
| **Plugin identity** | `SHA-256` of the [v0 canonical identity bytes](#canonical-identity-bytes-v0) (`name`, `version`, `schema`, `metadata`, optional implementation digest) |
| **Quota** | Multi-dimensional resource budget enforced by the runtime |
| **Railguard** | Host policy limiting goal spawning or capabilities |
| **Label** | Compact tag on a value (`Public` / `Internal` / `Confidential` / `Secret`) used for privacy policy |
| **Snapshot** | Serializable full (or incremental) run state, including the plugin identity map |
| **Tollana IR** | The core’s native instruction set; not WASM. Normative specification: [RFC 0002](bytecode.md) |

---

*End of RFC 0001.*
