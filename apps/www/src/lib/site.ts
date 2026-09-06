export const SITE_ORIGIN = 'https://tollana.ai'
export const GITHUB_REPO = 'https://github.com/lykhonis/tollana'
export const ARCHITECTURE_URL =
  'https://github.com/lykhonis/tollana/blob/main/docs/architecture.md'

export const copy = {
  siteName: 'Tollana',
  title: 'Tollana — a runtime for agents that last',
  description:
    'A runtime for long-running AI agents that can pause, move, and pick up exactly where they left off. Durable runs, a complete audit trail, and a host you can swap without rewriting the agent.',
  line: 'Agents that pause, move, and pick up exactly where they left off.',
  lede: 'Durable runs, a complete audit trail, and a host you can swap without rewriting the agent. Guests start with nothing.',
  qualitiesHeading: 'What it is',
  qualities: [
    {
      label: 'Durable',
      line: 'Stop mid-work, restore on another machine, continue without loss. Snapshots are exact, not best effort.',
    },
    {
      label: 'Portable',
      line: 'The same agent runs from a phone or embedded device through a laptop to a cluster or the edge.',
    },
    {
      label: 'Modular',
      line: 'Models, files, networks, and other powers are not baked in. The host attaches only what that run may use.',
    },
    {
      label: 'Swappable',
      line: 'Change providers, local vs cloud models, storage, or policy without rewriting the agent.',
    },
    {
      label: 'Auditable',
      line: 'Meaningful steps are journaled as they happen. Replay, time-travel, and inspect instead of reconstructing logs.',
    },
    {
      label: 'Least privilege',
      line: 'Guests start with nothing. Every power is an explicit, attenuable capability.',
    },
    {
      label: 'Accountable',
      line: 'Budgets for compute and tokens are first-class. Cost is attributable per run and per sub-goal.',
    },
    {
      label: 'Untrusted by default',
      line: 'Model-generated code runs in isolation, on a tight budget, with only the capabilities you pass in.',
    },
  ],
  buildHeading: 'How it is built',
  build: [
    {
      label: 'Host',
      line: 'Resolves plugins, grants capabilities, and holds policy. You can replace the host without rewriting the guest.',
    },
    {
      label: 'Plugins',
      line: 'Equal packages — models, files, network, clocks, goals. Identity is content-hashed so a restore cannot silently bind a different version.',
    },
    {
      label: 'Guest',
      line: 'The agent program. No ambient authority. It only sees what the host placed in its hands.',
    },
  ],
  sourceHeading: 'Source',
  sourceBody:
    'Tollana is Apache-2.0. The architecture RFC and the runtime live in the same repository.',
  sourceCta: 'View the source',
  architectureCta: 'Read the architecture',
  namedFor: 'The name is from Stargate.',
  copyright: '© 2026 Tollana',
  license: 'Apache-2.0',
  notFoundTitle: 'Page not found',
  notFoundBody: 'This address is not part of the site.',
  notFoundHome: 'Back home',
  errorTitle: 'Something went wrong',
  errorHome: 'Back home',
} as const
