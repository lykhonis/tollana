import { MachineFrame } from '@/components/MachineFrame'
import { MachineLegend } from '@/components/MachineLegend'
import { MachineLink } from '@/components/MachineLink'
import { copy, plugins } from '@/lib/site'

export function Machine() {
  const { host, core, guest, register, run } = copy.machine

  return (
    <section aria-labelledby="machine-heading">
      <h2 id="machine-heading" className="text-accent text-sm">
        {copy.machineHeading}
      </h2>
      <div className="mt-8">
        <MachineFrame
          title={host.label}
          note={host.sketch}
          captionOn="canvas"
          fill="bg-canvas-2"
        >
          <ul className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-sm">
            {plugins.map((name) => (
              <li key={name}>{name}</li>
            ))}
          </ul>
        </MachineFrame>

        <MachineLink label={register} />

        <MachineFrame
          title={core.label}
          note={core.sketch}
          captionOn="canvas"
          fill="bg-canvas"
        >
          <ul className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 text-sm sm:grid-cols-3">
            {core.duties.map((duty) => (
              <li key={duty}>{duty}</li>
            ))}
          </ul>
        </MachineFrame>

        <MachineLink label={run} />

        <MachineFrame
          title={guest.label}
          note={guest.sketch}
          captionOn="canvas"
          fill="bg-canvas-2/50"
        />
      </div>
      <MachineLegend />
    </section>
  )
}
