import { ARCHITECTURE_URL, GITHUB_REPO, SITE_ORIGIN, copy } from '@/lib/site'

export function renderLlmsTxt() {
  const lines = [
    `# ${copy.siteName}`,
    '',
    `> ${copy.description}`,
    '',
    copy.line,
    '',
    copy.lede,
    '',
    `## ${copy.qualitiesHeading}`,
    '',
    ...copy.qualities.map((item) => `- **${item.label}:** ${item.line}`),
    '',
    `## ${copy.buildHeading}`,
    '',
    ...copy.build.map((item) => `- **${item.label}:** ${item.line}`),
    '',
    `## ${copy.sourceHeading}`,
    '',
    copy.sourceBody,
    '',
    `- Source: ${GITHUB_REPO}`,
    `- Architecture: ${ARCHITECTURE_URL}`,
    `- License: ${copy.license}`,
    '',
    '## Files',
    '',
    `- Home: ${SITE_ORIGIN}/`,
    `- Sitemap: ${SITE_ORIGIN}/sitemap.xml`,
    `- This file: ${SITE_ORIGIN}/llms.txt`,
    '',
  ]

  return `${lines.join('\n').trim()}\n`
}
