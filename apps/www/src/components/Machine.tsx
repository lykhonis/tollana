import { copy, plugins } from '@/lib/site'

export function Machine() {
  const { host, plugins: pluginLayer, guest } = copy.machine

  return (
    <section aria-labelledby="machine-heading">
      <h2 id="machine-heading" className="text-accent text-sm">
        {copy.machineHeading}
      </h2>
      <div className="border-hairline bg-canvas-2/50 mt-6 border p-3 sm:p-4">
        <p className="text-xs tracking-wide">{host.label}</p>
        <p className="text-ink-soft mt-2 max-w-xl text-sm leading-relaxed">
          {host.line}
        </p>
        <div className="border-hairline bg-canvas mt-4 border p-3 sm:p-4">
          <p className="text-xs tracking-wide">{pluginLayer.label}</p>
          <p className="text-ink-soft mt-2 max-w-xl text-sm leading-relaxed">
            {pluginLayer.line}
          </p>
          <ul className="mt-4 flex flex-wrap gap-2">
            {plugins.map((name) => (
              <li
                key={name}
                className="border-hairline px-2 py-1 font-mono text-xs"
              >
                {name}
              </li>
            ))}
          </ul>
          <div className="border-hairline mt-4 border border-dashed p-3 sm:p-4">
            <p className="text-xs tracking-wide">{guest.label}</p>
            <p className="text-ink-soft mt-2 max-w-xl text-sm leading-relaxed">
              {guest.line}
            </p>
          </div>
        </div>
      </div>
    </section>
  )
}
