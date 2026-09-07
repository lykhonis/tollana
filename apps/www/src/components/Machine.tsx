import { MachineLegend } from '@/components/MachineLegend'
import { renderMachineDiagram } from '@/lib/machine-diagram'
import { copy } from '@/lib/site'

export function Machine() {
  return (
    <section aria-labelledby="machine-heading">
      <h2 id="machine-heading" className="text-accent text-sm">
        {copy.machineHeading}
      </h2>
      <div className="border-hairline bg-canvas-2/40 mt-6 overflow-x-auto border p-3 sm:p-4">
        <pre className="text-ink min-w-max font-mono text-[11px] leading-[1.45] sm:text-xs">
          {renderMachineDiagram()}
        </pre>
      </div>
      <MachineLegend />
    </section>
  )
}
