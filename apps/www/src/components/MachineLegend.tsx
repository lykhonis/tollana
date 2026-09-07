import { copy } from '@/lib/site'

const layers = [copy.machine.host, copy.machine.core, copy.machine.guest]

export function MachineLegend() {
  return (
    <ul className="mt-6 space-y-3">
      {layers.map((layer) => (
        <li key={layer.label}>
          <p className="text-sm font-medium">{layer.label}</p>
          <p className="text-ink-soft mt-1 max-w-xl text-sm leading-relaxed">
            {layer.line}
          </p>
        </li>
      ))}
    </ul>
  )
}
