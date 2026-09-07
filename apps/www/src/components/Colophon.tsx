import { GITHUB_REPO, copy } from '@/lib/site'

export function Colophon() {
  return (
    <footer className="text-ink-soft pb-[env(safe-area-inset-bottom)] text-sm">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-2 px-[clamp(1rem,4vw,2.5rem)] py-8 sm:flex-row sm:items-baseline sm:justify-between">
        <p>
          {copy.copyright}
          <span className="mx-2" aria-hidden="true">
            ·
          </span>
          {copy.license}
        </p>
        <p>
          <a
            href={GITHUB_REPO}
            className="hover:text-ink"
            rel="noreferrer"
            target="_blank"
          >
            {copy.sourceCta}
          </a>
          <span className="mx-2" aria-hidden="true">
            ·
          </span>
          {copy.namedFor}
        </p>
      </div>
    </footer>
  )
}
