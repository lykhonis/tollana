import { MachineFrame } from '@/components/MachineFrame'
import { MachineLegend } from '@/components/MachineLegend'
import { copy, plugins } from '@/lib/site'

export function Machine() {
  return (
    <section aria-labelledby="machine-heading">
      <h2 id="machine-heading" className="text-accent text-sm">
        {copy.machineHeading}
      </h2>
      <div className="mt-8">
        <MachineFrame
          title={copy.machine.host.label}
          note={copy.machine.host.sketch}
          captionOn="canvas"
          fill="bg-canvas-2"
        >
          <div className="mt-3">
            <MachineFrame
              title={copy.machine.plugins.label}
              captionOn="canvas-2"
              fill="bg-canvas"
            >
              <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
                {plugins.map((name) => (
                  <li key={name}>{name}</li>
                ))}
              </ul>
              <div className="mt-3">
                <MachineFrame
                  title={copy.machine.guest.label}
                  note={copy.machine.guest.sketch}
                  captionOn="canvas"
                  fill="bg-canvas-2/50"
                />
              </div>
            </MachineFrame>
          </div>
        </MachineFrame>
      </div>
      <MachineLegend />
    </section>
  )
}
