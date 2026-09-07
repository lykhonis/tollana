import { describe, expect, it } from 'vitest'
import { pageHead, softwareJsonLd } from '@/lib/seo'
import { GITHUB_REPO, SITE_ORIGIN, copy } from '@/lib/site'

describe('www SEO', () => {
  it('canonicalises the apex and indexes the page', () => {
    const head = pageHead()
    expect(head.meta).toContainEqual({
      name: 'robots',
      content: 'index, follow',
    })
    expect(head.links).toContainEqual({
      rel: 'canonical',
      href: `${SITE_ORIGIN}/`,
    })
    expect(head.meta).toContainEqual({
      property: 'og:image',
      content: `${SITE_ORIGIN}/og.jpg`,
    })
    expect(head.meta).toContainEqual({
      name: 'twitter:card',
      content: 'summary_large_image',
    })
    expect(head.meta).toContainEqual({ title: copy.title })
    expect(head.meta).toContainEqual({
      name: 'theme-color',
      content: '#F6F3EC',
    })
  })

  it('exposes the project as structured data', () => {
    const graph = softwareJsonLd()
    const json = JSON.stringify(graph)
    expect(json).toContain('SoftwareSourceCode')
    expect(json).toContain('Organization')
    expect(json).toContain('WebSite')
    expect(json).toContain(copy.siteName)
    expect(json).toContain(GITHUB_REPO)
    expect(json).toContain('LICENSE-2.0')
    expect(json).toContain(`${SITE_ORIGIN}/logo.png`)
  })
})
