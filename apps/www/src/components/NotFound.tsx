import { Link } from '@tanstack/react-router'

export function NotFound() {
  return (
    <section>
      <h1>Page not found</h1>
      <p>This address is not part of the site.</p>
      <Link to="/">Back home</Link>
    </section>
  )
}
