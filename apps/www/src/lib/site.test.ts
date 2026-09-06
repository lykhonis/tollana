import { describe, expect, it } from 'vitest'
import { GITHUB_REPO, SITE_ORIGIN, copy } from '@/lib/site'

describe('www copy', () => {
  it('points at the apex and GitHub repo', () => {
    expect(SITE_ORIGIN).toBe('https://tollana.ai')
    expect(GITHUB_REPO).toBe('https://github.com/lykhonis/tollana')
    expect(copy.siteName).toBe('Tollana')
    expect(copy.copyright).toContain('2026')
    expect(copy.license).toBe('Apache-2.0')
  })
})
