import { describe, expect, it } from 'vitest'
import { renderLlmsTxt } from '@/lib/llms-txt'
import { ARCHITECTURE_URL, GITHUB_REPO, SITE_ORIGIN } from '@/lib/site'

describe('renderLlmsTxt', () => {
  it('describes Tollana as a journaled runtime', () => {
    const text = renderLlmsTxt()
    expect(text).toContain('# Tollana')
    expect(text).toContain('pause, move, and pick up')
    expect(text).toContain('Durable')
    expect(text).toContain('Untrusted by default')
    expect(text).toContain('host')
    expect(text).toContain('plugins')
    expect(text).toContain('guest')
    expect(text).toContain(GITHUB_REPO)
    expect(text).toContain(ARCHITECTURE_URL)
    expect(text).toContain('Apache-2.0')
    expect(text).toContain(`${SITE_ORIGIN}/`)
    expect(text).toContain(`${SITE_ORIGIN}/llms.txt`)
    expect(text).toContain(`${SITE_ORIGIN}/sitemap.xml`)
  })
})
