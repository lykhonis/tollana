import { describe, expect, it } from 'vitest'
import { brand } from '@/lib/mark'

describe('brand mark', () => {
  it('keeps the journal, seal, and continue geometry', () => {
    expect(brand.markPath).toContain('M1 4h2v2H1z')
    expect(brand.markPath).toContain('M9 8h6v8H9z')
    expect(brand.markPath).toContain('M20 9h2v2h-2z')
    expect(brand.canvas).toBe('#F6F3EC')
    expect(brand.accent).toBe('#0B6E99')
  })
})
