import { describe, expect, it } from 'vitest'
import { GITHUB_REPO, SITE_ORIGIN, copy } from '@/lib/site'

describe('www copy', () => {
  it('points at the apex and GitHub repo', () => {
    expect(SITE_ORIGIN).toBe('https://tollana.ai')
    expect(GITHUB_REPO).toBe('https://github.com/lykhonis/tollana')
    expect(copy.siteName).toBe('Tollana')
    expect(copy.title).toContain('runtime for agents that last')
    expect(copy.line).toContain('pause')
    expect(copy.lede).toContain('Guests start with nothing')
    expect(copy.qualities.map((item) => item.label)).toEqual([
      'Durable',
      'Portable',
      'Modular',
      'Swappable',
      'Auditable',
      'Least privilege',
      'Accountable',
      'Untrusted by default',
    ])
    expect(copy.build.map((item) => item.label)).toEqual([
      'Host',
      'Plugins',
      'Guest',
    ])
    expect(copy.namedFor).toContain('Stargate')
    expect(copy.copyright).toContain('2026')
    expect(copy.license).toBe('Apache-2.0')
  })
})
