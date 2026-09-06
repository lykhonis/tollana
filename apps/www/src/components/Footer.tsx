import { PixelIcon } from '@/components/PixelIcon'
import { GITHUB_REPO, copy } from '@/lib/site'

export function Footer() {
  return (
    <footer className="border-hairline mt-auto border-t pb-[env(safe-area-inset-bottom)]">
      <div className="text-ink-soft mx-auto flex w-full max-w-5xl flex-col gap-4 px-[clamp(1rem,4vw,3rem)] py-5 text-sm sm:flex-row sm:items-center sm:justify-between">
        <p className="font-mono">{copy.copyright}</p>
        <div className="flex flex-wrap items-center gap-1">
          <span className="px-2">{copy.license}</span>
          <a
            href={GITHUB_REPO}
            className="hover:text-ink inline-flex size-11 items-center justify-center"
            rel="noreferrer"
            target="_blank"
            aria-label="GitHub"
          >
            <PixelIcon name="github" className="size-6" />
          </a>
        </div>
      </div>
    </footer>
  )
}
