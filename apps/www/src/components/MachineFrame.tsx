import type { ReactNode } from 'react'

export function MachineFrame({
  title,
  note,
  fill,
  children,
}: {
  title: string
  note?: string
  fill: string
  children?: ReactNode
}) {
  return (
    <fieldset
      className={`border-hairline m-0 min-w-0 rounded-lg border px-3 pt-1 pb-3 ${fill}`}
    >
      <legend className="text-accent ml-1 px-1.5 text-xs">{title}</legend>
      {note ? (
        <p className="text-ink-soft text-xs leading-relaxed">{note}</p>
      ) : null}
      {children}
    </fieldset>
  )
}
