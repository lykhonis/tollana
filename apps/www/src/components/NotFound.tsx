import { Link } from '@tanstack/react-router'
import { copy } from '@/lib/site'

export function NotFound() {
  return (
    <section className="mx-auto w-full max-w-3xl px-[clamp(1rem,4vw,2.5rem)] py-16 sm:py-24">
      <p className="text-accent text-sm">404</p>
      <h1 className="mt-4 text-2xl font-medium">{copy.notFoundTitle}</h1>
      <p className="text-ink-soft mt-3 max-w-md leading-relaxed">
        {copy.notFoundBody}
      </p>
      <p className="mt-8">
        <Link to="/" className="text-accent hover:underline">
          {copy.notFoundHome}
        </Link>
      </p>
    </section>
  )
}
