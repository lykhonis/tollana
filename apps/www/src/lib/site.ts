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
  lede: 'A run is a MachineState you can suspend, restore, replay, and meter. Guests start with nothing. Every power is a Capability the host grants.',
  status: 'exact snapshots · no ambient authority',
  journalHeading: 'journal',
  journal: [
    {
      cite: 'SnapshotTaken',
      label: 'Durable',
      line: 'Stop mid-work, restore on another machine, continue without loss. SnapshotTaken and SnapshotRestored record an exact MachineState, not a best-effort dump.',
    },
    {
      cite: 'MachineState',
      label: 'Portable',
      line: 'The same guest runs from a phone or embedded device through a laptop to a cluster or the edge. The host changes; the MachineState does not.',
    },
    {
      cite: 'host.invoke',
      label: 'Modular',
      line: 'Models, files, networks, and other powers are not baked into the core. The only host-call opcode is host.invoke; the host attaches only what that run may use.',
    },
    {
      cite: 'identityHash',
      label: 'Swappable',
      line: 'Change providers, local vs cloud models, storage, or policy without rewriting the agent. Restore matches identityHash; it will not silently bind a different plugin version.',
    },
    {
      cite: 'InstructionStepped',
      label: 'Auditable',
      line: 'Meaningful steps are journaled as they happen — InstructionStepped, HostCallSuspended, HostCallResumed. Replay and inspect instead of reconstructing leftover logs.',
    },
    {
      cite: 'Capability',
      label: 'Least privilege',
      line: 'Guests start with nothing. Every power is an explicit Capability. Using a null or forged handle journals InvalidCapabilityUse and traps.',
    },
    {
      cite: 'QuotaConsumed',
      label: 'Accountable',
      line: 'Budgets for compute, tokens, and host.invoke count are first-class. QuotaConsumed, QuotaExhausted, and QuotaAdded are attributable per run.',
    },
    {
      cite: 'code.run',
      label: 'Untrusted by default',
      line: 'Model-generated code runs through code.run as an isolated child MachineState, on a tight budget, with only the capabilities you pass in.',
    },
  ],
  machineHeading: 'machine',
  machine: {
    host: {
      label: 'host',
      line: 'Resolves plugins, grants Capability values, and holds policy. Replace the host without rewriting the guest.',
    },
    plugins: {
      label: 'plugins',
      line: 'Equal packages. Each binding stores identityHash so a restore cannot silently bind a different version.',
    },
    guest: {
      label: 'guest',
      line: 'The agent program in a MachineState. No ambient authority. It only sees what the host placed in its hands, through host.invoke.',
    },
  },
  sourceCta: 'source',
  architectureCta: 'architecture',
  sourceBody:
    'Apache-2.0. The architecture RFC, Tollana IR, and the runtime live in the same repository.',
  namedFor: 'Named from Stargate.',
  copyright: '© 2026 Tollana',
  license: 'Apache-2.0',
  notFoundTitle: 'No run at this address',
  notFoundBody: 'This path is not in the journal.',
  notFoundHome: 'Back home',
  errorTitle: 'The run trapped',
  errorHome: 'Back home',
} as const
