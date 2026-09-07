import { copy, plugins } from '@/lib/site'

function pad(text: string, width: number) {
  if (text.length >= width) {
    return text.slice(0, width)
  }
  return `${text}${' '.repeat(width - text.length)}`
}

function box(title: string, lines: string[], inner: number) {
  const label = ` ${title} `
  const fill = Math.max(0, inner - label.length)
  const top = `┌${label}${'─'.repeat(fill)}┐`
  const bottom = `└${'─'.repeat(inner)}┘`
  const body = lines.map((line) => `│${pad(line, inner)}│`)
  return [top, ...body, bottom]
}

function pluginRows(names: readonly string[], columns: number) {
  const cell = Math.max(...names.map((name) => name.length))
  const rows: string[] = []
  for (let i = 0; i < names.length; i += columns) {
    const slice = names.slice(i, i + columns)
    rows.push(slice.map((name) => name.padEnd(cell, ' ')).join('  '))
  }
  return rows
}

export function renderMachineDiagram() {
  const guestNote = ` ${copy.machine.guest.sketch}`
  const guestInner = Math.max(
    22,
    copy.machine.guest.label.length + 4,
    guestNote.length,
  )
  const guest = box(copy.machine.guest.label, [guestNote], guestInner)

  const listed = pluginRows(plugins, 4)
  const pluginsInner = Math.max(
    guest[0].length + 2,
    copy.machine.plugins.label.length + 4,
    ...listed.map((line) => line.length + 2),
  )
  const pluginLines = [
    ...listed.map((line) => ` ${line}`),
    ...guest.map((line) => pad(` ${line}`, pluginsInner)),
  ]
  const pluginBox = box(copy.machine.plugins.label, pluginLines, pluginsInner)

  const hostNote = ` ${copy.machine.host.sketch}`
  const hostInner = Math.max(
    pluginBox[0].length + 2,
    copy.machine.host.label.length + 4,
    hostNote.length,
  )
  const hostLines = [
    hostNote,
    ...pluginBox.map((line) => pad(` ${line}`, hostInner)),
  ]

  return box(copy.machine.host.label, hostLines, hostInner).join('\n')
}
