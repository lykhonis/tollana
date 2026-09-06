import { Link } from '@tanstack/react-router'
import { copy } from '@/lib/site'

export function NotFound() {
  return (
    <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] py-16 sm:py-24">
      <p className="text-ink-soft font-mono text-xs tracking-widest uppercase">
        404
      </p>
      <h1 className="mt-3 text-3xl">{copy.notFoundTitle}</h1>
      <p className="text-ink-soft mt-4 max-w-md text-lg">{copy.notFoundBody}</p>
      <Link
        to="/"
        className="border-hairline hover:bg-canvas-2 mt-8 inline-flex min-h-11 items-center border px-4 py-2 text-sm sm:mt-10"
      >
        {copy.notFoundHome}
      </Link>
    </section>
  )
}
