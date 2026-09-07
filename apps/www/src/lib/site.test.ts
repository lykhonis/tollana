import { describe, expect, it } from 'vitest'
import { GITHUB_REPO, SITE_ORIGIN, copy, plugins } from '@/lib/site'

describe('www copy', () => {
  it('points at the apex and GitHub repo', () => {
    expect(SITE_ORIGIN).toBe('https://tollana.ai')
    expect(GITHUB_REPO).toBe('https://github.com/lykhonis/tollana')
    expect(copy.siteName).toBe('Tollana')
    expect(copy.title).toContain('runtime for agents that last')
    expect(copy.line).toContain('pause')
    expect(copy.lede).toContain('Guests start with nothing')
    expect(copy.status).toContain('no ambient authority')
  })

  it('lists qualities as a heading and a line', () => {
    expect(copy.journal.map((item) => item.label)).toEqual([
      'Durable',
      'Portable',
      'Modular',
      'Swappable',
      'Auditable',
      'Least privilege',
      'Accountable',
      'Untrusted by default',
    ])
  })

  it('describes the host, core, and guest', () => {
    expect(copy.machine.host.label).toBe('host')
    expect(copy.machine.host.sketch).toContain('grant')
    expect(copy.machine.core.label).toBe('core')
    expect(copy.machine.core.duties).toEqual([
      'interpret',
      'continuations',
      'snapshot',
      'journal',
      'quotas',
      'capabilities',
    ])
    expect(copy.machine.guest.label).toBe('guest')
    expect(copy.machine.guest.sketch).toBe('no ambient authority')
    expect(plugins).toContain('ai')
    expect(plugins).toContain('code')
    expect(copy.origin).toContain('Tollan')
    expect(copy.origin).toContain('Tollana')
    expect(copy.origin).toContain('Stargate')
    expect(copy.license).toBe('Apache-2.0')
  })
})
