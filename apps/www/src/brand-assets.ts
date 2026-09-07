import { writeFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'
import { brand, markSvgMarkup } from '@/lib/mark'

const publicDir = fileURLToPath(new URL('../public', import.meta.url))

function markSvg(size: number) {
  return Buffer.from(markSvgMarkup(size, { field: true, contrast: false }))
}

await writeFile(
  `${publicDir}/favicon.svg`,
  markSvgMarkup(32, { field: false, contrast: true }),
)
await sharp(markSvg(512)).png().toFile(`${publicDir}/logo.png`)
await sharp(markSvg(180)).png().toFile(`${publicDir}/apple-touch-icon.png`)

const badge = await sharp(markSvg(264)).png().toBuffer()
const label = await sharp(
  Buffer.from(`<svg xmlns="http://www.w3.org/2000/svg" width="420" height="80">
    <text x="0" y="55" fill="${brand.ring}" font-family="Geist Mono, ui-monospace, Menlo, monospace" font-size="42" letter-spacing="6">TOLLANA</text>
  </svg>`),
)
  .png()
  .toBuffer()

await sharp({
  create: {
    width: 1200,
    height: 630,
    channels: 3,
    background: brand.navy,
  },
})
  .composite([
    { input: badge, left: 160, top: 183 },
    { input: label, left: 450, top: 275 },
  ])
  .jpeg({ quality: 88 })
  .toFile(`${publicDir}/og.jpg`)

console.log('wrote favicon.svg, logo.png, apple-touch-icon.png, og.jpg')
