export const brand = {
  canvas: '#F6F3EC',
  canvas2: '#EBE6DC',
  ink: '#1A1D21',
  inkSoft: '#5C6570',
  accent: '#0B8F82',
  navy: '#0B1E3A',
  ring: '#FFFFFF',
  bowl: '#00E4C8',
} as const

/**
 * Lower crescent: inner disk A (12,12 r=6.5) minus cutting disk B (12,5.5 r=10).
 * First arc follows A along the bottom; second follows B so the cut dips (bowl).
 */
export const bowlPath =
  'M5.61 13.19A6.5 6.5 0 0 0 18.39 13.19A10 10 0 0 1 5.61 13.19Z'

export function markSvgMarkup(
  size: number,
  opts: { field: boolean; contrast: boolean },
) {
  const field = opts.field
    ? `<rect width="24" height="24" fill="${brand.navy}"/>`
    : ''
  const contrast = opts.contrast
    ? `<circle cx="12" cy="12" r="8.9" fill="none" stroke="${brand.navy}" stroke-width="0.8"/>`
    : ''
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24">
  ${field}
  ${contrast}
  <circle cx="12" cy="12" r="7.5" fill="none" stroke="${brand.ring}" stroke-width="2"/>
  <path fill="${brand.bowl}" d="${bowlPath}"/>
</svg>`
}
