import { describe, expect, it } from 'vitest'
import { brand } from '@/lib/mark'

describe('brand mark', () => {
  it('is a machine frame around an operand stack', () => {
    expect(brand.markPath).toContain('M4 2h16v2H4z')
    expect(brand.markPath).toContain('M7 16h10v2H7z')
    expect(brand.markPath).toContain('M9 7h6v2H9z')
    expect(brand.canvas).toBe('#F6F3EC')
    expect(brand.accent).toBe('#0B6E99')
  })
})
