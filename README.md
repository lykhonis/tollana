# Tollana

<img src="apps/www/public/favicon.svg" width="56" height="56" alt="Tollana mark">

[![tollana.ai](https://img.shields.io/website?url=https%3A%2F%2Ftollana.ai&up_message=live&down_message=down&label=tollana.ai)](https://tollana.ai)
[![Release](https://img.shields.io/github/v/release/lykhonis/tollana?include_prereleases)](https://github.com/lykhonis/tollana/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

A runtime for long-running AI agents that can pause, move, and pick up exactly where they left off.

Agents should survive host changes, stay inspectable after the fact, and never inherit more power than you granted. Tollana is built around that: durable runs, a complete audit trail, and a host that you can swap without rewriting the agent.

The name is from _Stargate_. Tollana is the world the Tollan rebuilt after sharing technology destroyed a neighboring civilization. They did not share it that way again.

The public site is [tollana.ai](https://tollana.ai).

## Why

Most agent stacks assume a process that stays up, a single model backend, and implicit access to tools. Real work is longer than a request, hops between devices, and has to be explained later—especially when cost, privacy, or generated code are in play.

Tollana treats a run as something you can **suspend, migrate, replay, and meter**, with **no ambient authority**.

| Quality                  | Meaning                                                                                                                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Durable**              | A run can stop mid-work, restore on another machine, and continue without loss of state. Snapshots are exact, not “best effort.”                                                     |
| **Portable**             | The same agent runs from a phone or embedded device through a laptop to a cluster or the edge. The host changes; the agent does not.                                                 |
| **Modular**              | Models, files, networks, and other powers are not baked into the runtime. The host attaches only what that run is allowed to use.                                                    |
| **Swappable**            | Switch providers, local vs cloud models, storage, or policy without rewriting agent code. Every plugin is an equal package; a restore will not silently bind to a different version. |
| **Auditable**            | Meaningful steps are journaled as they happen. Replay, time-travel, and inspect a run instead of reconstructing it from leftover logs.                                               |
| **Least privilege**      | Guests start with nothing. Every power is an explicit, attenuable capability. Sensitive data can stay on-device by policy.                                                           |
| **Accountable**          | Budgets—compute, tokens, and the rest—are first-class. Cost and usage are attributable per run and per sub-goal.                                                                     |
| **Untrusted by default** | Model-generated code runs in isolation, on a tight budget, with only the capabilities you pass in.                                                                                   |

A run has three layers. The **host** resolves plugins, grants capabilities, and holds policy. The **core** is an explicit stack-machine interpreter: it suspends, resumes, meters, journals, snapshots, and enforces grants—it does not know about models or files. The **guest** is the agent program; it only sees what the host placed in its hands.

## Docs

- [RFC 0001 — Architecture](docs/architecture.md)
- [RFC 0002 — Tollana IR v0](docs/bytecode.md)

## Contributing

This repository does **not** accept pull requests or external contributions right now. Issues and PRs are not a contribution path. Forks under Apache 2.0 are fine.

## License

[Apache License 2.0](LICENSE)
