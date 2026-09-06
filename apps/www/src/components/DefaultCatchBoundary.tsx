import type { ErrorComponentProps } from '@tanstack/react-router'
import { Link } from '@tanstack/react-router'

export function DefaultCatchBoundary({ error }: ErrorComponentProps) {
  return (
    <section>
      <h1>Something went wrong</h1>
      <p>{error instanceof Error ? error.message : 'Unexpected error'}</p>
      <Link to="/">Back home</Link>
    </section>
  )
}
