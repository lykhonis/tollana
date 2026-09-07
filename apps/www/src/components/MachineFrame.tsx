import type { ReactNode } from 'react'

const captionFill = {
  canvas: 'bg-canvas',
  'canvas-2': 'bg-canvas-2',
} as const

export function MachineFrame({
  title,
  note,
  captionOn,
  fill,
  children,
}: {
  title: string
  note?: string
  captionOn: keyof typeof captionFill
  fill: string
  children?: ReactNode
}) {
  return (
    <div
      className={`border-hairline relative rounded-lg border px-3 pt-4 pb-3 ${fill}`}
    >
      <p
        className={`text-accent absolute -top-2 left-3 px-1.5 text-xs ${captionFill[captionOn]}`}
      >
        {title}
      </p>
      {note ? (
        <p className="text-ink-soft text-xs leading-relaxed">{note}</p>
      ) : null}
      {children}
    </div>
  )
}
