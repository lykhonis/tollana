import { describe, expect, it } from 'vitest'
import { renderMachineDiagram } from '@/lib/machine-diagram'
import { plugins } from '@/lib/site'

describe('renderMachineDiagram', () => {
  it('nests host, plugins, and guest in a box-drawing sketch', () => {
    const diagram = renderMachineDiagram()
    const lines = diagram.split('\n')
    expect(lines[0]?.startsWith('┌ host')).toBe(true)
    expect(lines.at(-1)?.startsWith('└')).toBe(true)
    expect(diagram).toContain('┌ plugins')
    expect(diagram).toContain('┌ guest')
    expect(diagram).toContain('grants · policy · plugin resolution')
    expect(diagram).toContain('no ambient authority')
    for (const name of plugins) {
      expect(diagram).toContain(name)
    }
    const width = lines[0]?.length
    expect(lines.every((line) => line.length === width)).toBe(true)
    expect(lines[0]?.endsWith('┐')).toBe(true)
    expect(lines.at(-1)?.endsWith('┘')).toBe(true)
    expect(diagram).toMatch(/┌ plugins [─]+┐/)
    expect(diagram).toMatch(/┌ guest [─]+┐/)
  })
})
