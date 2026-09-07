export const SITE_ORIGIN = 'https://tollana.ai'
export const GITHUB_REPO = 'https://github.com/lykhonis/tollana'
export const ARCHITECTURE_URL =
  'https://github.com/lykhonis/tollana/blob/main/docs/architecture.md'

/** Well-known plugin package names from packages/tollana-host/schemas. */
export const plugins = [
  'ai',
  'clock',
  'code',
  'context',
  'fs',
  'goal',
  'net',
  'random',
] as const

export const copy = {
  siteName: 'Tollana',
  title: 'Tollana — a runtime for agents that last',
  description:
    'A runtime for long-running AI agents that can pause, move, and pick up exactly where they left off. Durable runs, a complete audit trail, and a host you can swap without rewriting the agent.',
  line: 'Agents that pause, move, and pick up exactly where they left off.',
  lede: 'A run is something you can suspend, restore, replay, and meter. Guests start with nothing. Every power is a grant.',
  status: 'exact snapshots · no ambient authority',
  journalHeading: 'journal',
  journal: [
    {
      label: 'Durable',
      line: 'Stop mid-work, restore on another machine, continue without loss. Snapshots are exact, not a best-effort dump.',
    },
    {
      label: 'Portable',
      line: 'The same guest runs from a phone or embedded device through a laptop to a cluster or the edge. The host changes; the agent does not.',
    },
    {
      label: 'Modular',
      line: 'Models, files, networks, and other powers are not baked into the core. The host attaches only what that run may use.',
    },
    {
      label: 'Swappable',
      line: 'Change providers, local vs cloud models, storage, or policy without rewriting the agent. A restore will not silently bind a different plugin version.',
    },
    {
      label: 'Auditable',
      line: 'Meaningful steps are journaled as they happen. Replay and inspect instead of reconstructing leftover logs.',
    },
    {
      label: 'Least privilege',
      line: 'Guests start with nothing. Every power is an explicit, attenuable capability. Sensitive data can stay on-device by policy.',
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
  machineHeading: 'machine',
  machine: {
    host: {
      label: 'host',
      sketch: 'resolve · hash · grant',
      line: 'Resolves plugins, grants capabilities, and holds policy. Replace the host without rewriting the guest.',
    },
    core: {
      label: 'core',
      sketch: 'stack machine · exact snapshots',
      line: 'An explicit interpreter. It suspends, resumes, meters, journals, snapshots, and enforces grants. It does not know about models or files.',
      duties: [
        'interpret',
        'continuations',
        'snapshot',
        'journal',
        'quotas',
        'capabilities',
      ],
    },
    guest: {
      label: 'guest',
      sketch: 'no ambient authority',
      line: 'The agent program. It only sees what the host placed in its hands.',
    },
    register: 'register',
    run: 'run',
  },
  sourceCta: 'source',
  architectureCta: 'architecture',
  sourceBody:
    'Apache-2.0. The architecture RFC and the runtime live in the same repository.',
  namedFor: 'Named from Stargate.',
  copyright: '© 2026 Tollana',
  license: 'Apache-2.0',
  notFoundTitle: 'No run at this address',
  notFoundBody: 'This path is not in the journal.',
  notFoundHome: 'Back home',
  errorTitle: 'The run trapped',
  errorHome: 'Back home',
} as const
