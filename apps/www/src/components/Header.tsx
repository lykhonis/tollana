import { Link } from '@tanstack/react-router'
import { PixelIcon } from '@/components/PixelIcon'
import { GITHUB_REPO, copy } from '@/lib/site'

export function Header() {
  return (
    <header className="border-hairline bg-canvas/95 sticky top-0 z-50 border-b pt-[env(safe-area-inset-top)] backdrop-blur-md">
      <div className="mx-auto flex w-full max-w-5xl items-center justify-between gap-3 px-[clamp(1rem,4vw,3rem)] py-2 sm:py-3">
        <Link
          to="/"
          className="flex min-w-0 items-center gap-2 text-sm tracking-wide"
          aria-label="Home"
        >
          <PixelIcon name="mark" className="text-accent size-6 shrink-0" />
          <span className="truncate font-mono font-medium">
            {copy.siteName}
          </span>
        </Link>
        <nav>
          <ul className="flex font-mono text-sm">
            <li>
              <a
                href={GITHUB_REPO}
                className="text-ink-soft hover:text-ink hover:bg-canvas-2 inline-flex min-h-11 items-center gap-2 px-3"
                rel="noreferrer"
                target="_blank"
              >
                <PixelIcon name="github" className="size-5" />
                GitHub
              </a>
            </li>
          </ul>
        </nav>
      </div>
    </header>
  )
}
