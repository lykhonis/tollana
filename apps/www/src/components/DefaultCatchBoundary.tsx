import type { ErrorComponentProps } from '@tanstack/react-router'
import { Link } from '@tanstack/react-router'
import { copy } from '@/lib/site'

export function DefaultCatchBoundary({ error }: ErrorComponentProps) {
  return (
    <section className="mx-auto w-full max-w-3xl px-[clamp(1rem,4vw,2.5rem)] py-16 sm:py-24">
      <p className="text-accent text-sm">trap</p>
      <h1 className="mt-4 text-2xl font-medium">{copy.errorTitle}</h1>
      <p className="text-ink-soft mt-3 max-w-xl text-sm leading-relaxed">
        {error instanceof Error ? error.message : 'Unexpected error'}
      </p>
      <p className="mt-8">
        <Link to="/" className="text-accent hover:underline">
          {copy.errorHome}
        </Link>
      </p>
    </section>
  )
}
