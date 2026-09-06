import { GITHUB_REPO, SITE_ORIGIN, copy } from '@/lib/site'

export function pageHead(opts?: {
  path?: string
  title?: string
  description?: string
}) {
  const path = opts?.path ?? '/'
  const canonical = `${SITE_ORIGIN}${path === '/' ? '/' : path}`
  const title = opts?.title ?? copy.title
  const description = opts?.description ?? copy.description
  const ogImage = `${SITE_ORIGIN}/og.jpg`

  return {
    meta: [
      { title },
      { name: 'description', content: description },
      { name: 'robots', content: 'index, follow' },
      { name: 'theme-color', content: '#F6F3EC' },
      { property: 'og:type', content: 'website' },
      { property: 'og:site_name', content: copy.siteName },
      { property: 'og:url', content: canonical },
      { property: 'og:title', content: title },
      { property: 'og:description', content: description },
      { property: 'og:locale', content: 'en_GB' },
      { property: 'og:image', content: ogImage },
      { property: 'og:image:width', content: '1200' },
      { property: 'og:image:height', content: '630' },
      { property: 'og:image:alt', content: copy.siteName },
      { name: 'twitter:card', content: 'summary_large_image' },
      { name: 'twitter:title', content: title },
      { name: 'twitter:description', content: description },
      { name: 'twitter:image', content: ogImage },
    ],
    links: [
      { rel: 'canonical', href: canonical },
      { rel: 'alternate', hrefLang: 'en', href: canonical },
      { rel: 'alternate', hrefLang: 'x-default', href: canonical },
    ],
  }
}

export function softwareJsonLd() {
  const orgId = `${SITE_ORIGIN}/#organization`
  const softwareId = `${SITE_ORIGIN}/#software`
  return {
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'Organization',
        '@id': orgId,
        name: copy.siteName,
        url: `${SITE_ORIGIN}/`,
        logo: {
          '@type': 'ImageObject',
          url: `${SITE_ORIGIN}/logo.png`,
        },
      },
      {
        '@type': 'SoftwareSourceCode',
        '@id': softwareId,
        name: copy.siteName,
        description: copy.description,
        url: `${SITE_ORIGIN}/`,
        codeRepository: GITHUB_REPO,
        license: 'https://www.apache.org/licenses/LICENSE-2.0',
        programmingLanguage: 'Rust',
        runtimePlatform: 'Tollana',
        applicationCategory: 'DeveloperApplication',
        creator: { '@id': orgId },
      },
      {
        '@type': 'WebSite',
        '@id': `${SITE_ORIGIN}/#website`,
        url: `${SITE_ORIGIN}/`,
        name: copy.siteName,
        description: copy.description,
        inLanguage: 'en',
        publisher: { '@id': orgId },
      },
    ],
  }
}
