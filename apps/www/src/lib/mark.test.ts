import { describe, expect, it } from 'vitest'
import { bowlPath, brand, markSvgMarkup } from '@/lib/mark'

describe('brand mark', () => {
  it('is a white ring and cyan crescent', () => {
    expect(brand.navy).toBe('#0B1E3A')
    expect(brand.ring).toBe('#FFFFFF')
    expect(brand.bowl).toBe('#00E4C8')
    expect(bowlPath).toContain('A6.5 6.5')
    expect(bowlPath).toContain('A10 10')
    const onPage = markSvgMarkup(32, { field: false, contrast: true })
    expect(onPage).not.toContain(`<rect`)
    expect(onPage).toContain(brand.navy)
    const onField = markSvgMarkup(32, { field: true, contrast: false })
    expect(onField).toContain(`fill="${brand.navy}"`)
  })
})
