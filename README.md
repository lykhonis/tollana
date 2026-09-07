# Tollana

[![CI](https://github.com/lykhonis/tollana/actions/workflows/ci.yml/badge.svg)](https://github.com/lykhonis/tollana/actions/workflows/ci.yml)
[![tollana.ai](https://img.shields.io/website?url=https%3A%2F%2Ftollana.ai&up_message=live&down_message=down&label=tollana.ai)](https://tollana.ai)
[![Release](https://img.shields.io/github/v/release/lykhonis/tollana?include_prereleases)](https://github.com/lykhonis/tollana/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

A runtime for long-running AI agents that can pause, move, and pick up exactly where they left off.

Agents should survive host changes, stay inspectable after the fact, and never inherit more power than you granted. Tollana is built around that: durable runs, a complete audit trail, and a host that you can swap without rewriting the agent.

The name is from _Stargate_.

The public site is [tollana.ai](https://tollana.ai) (`apps/www`). The mark is a ring with a remaining bowl: a continuation you can pick up.

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

## Development

Nx is the only entry. Requires **Node 22** and **pnpm**. After `pnpm install`, git hooks format and lint JavaScript on commit, run Rust `format` and `lint` when Rust files are staged, and run `format`, `lint`, `check`, `test`, and `typecheck` on push (`--parallel=1`). Bypass with `git commit --no-verify` / `git push --no-verify`.

```text
pnpm install
pnpm nx run-many -t format,lint,check,test --projects=tollana-core,tollana-host --parallel=1
pnpm nx run-many -t format,lint,typecheck,test,build --projects=www
pnpm nx dev www                # landing site → http://localhost:3000
pnpm nx preview www
```

## Deploy

The public site is `apps/www`, a TanStack Start Worker (`tollana-www`) on **https://tollana.ai**. Push to `main`. After rust and www CI are green, GitHub Actions deploys `www` only when Nx considers it affected, then purges the `tollana.ai` cache.

Repository secret: `CLOUDFLARE_API_TOKEN` (Edit Cloudflare Workers). Repository variables: `CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_ZONE_ID`.

The Worker binds the **apex only**. `www.tollana.ai` is a proxied CNAME to the apex plus a zone 301 (path and query preserved). HTTP → HTTPS is Always Use HTTPS plus a Redirect Rule. HSTS is on (one year, includeSubDomains). Bot Fight Mode, Cloudflare-managed robots.txt, and Block AI Bots stay **off** so classic crawlers and AI providers can fetch `/`, `/robots.txt`, `/sitemap.xml`, and `/llms.txt`.

Manual deploy (Wrangler login or `CLOUDFLARE_API_TOKEN`):

```text
pnpm nx deploy www
```

## Contributing

This repository does **not** accept pull requests or external contributions right now. Issues and PRs are not a contribution path. Forks under Apache 2.0 are fine.

## License

[Apache License 2.0](LICENSE)
