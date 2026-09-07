export function MachineLink({ label }: { label: string }) {
  return (
    <div
      className="text-ink-soft flex flex-col items-center py-1"
      aria-hidden="true"
    >
      <span className="bg-ink/20 block h-3 w-px" />
      <span className="py-1 text-[11px] tracking-wide">{label}</span>
      <span className="bg-ink/20 block h-3 w-px" />
    </div>
  )
}
