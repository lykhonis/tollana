import { Link } from '@tanstack/react-router'
import { BrandMark } from '@/components/BrandMark'
import { copy } from '@/lib/site'

export function Masthead() {
  return (
    <p className="pt-[env(safe-area-inset-top)]">
      <Link to="/" className="inline-flex items-center gap-3" aria-label="Home">
        <BrandMark className="size-8 shrink-0" />
        <span className="text-sm font-medium">{copy.siteName}</span>
      </Link>
    </p>
  )
}
