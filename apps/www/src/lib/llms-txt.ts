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
    `## ${copy.journalHeading}`,
    '',
    ...copy.journal.map((item) => `- **${item.label}:** ${item.line}`),
    '',
    `## ${copy.machineHeading}`,
    '',
    `- **${copy.machine.host.label}:** ${copy.machine.host.line}`,
    `- **${copy.machine.plugins.label}:** ${copy.machine.plugins.line}`,
    `- **${copy.machine.guest.label}:** ${copy.machine.guest.line}`,
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
