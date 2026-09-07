import { Journal } from '@/components/Journal'
import { Machine } from '@/components/Machine'
import { ARCHITECTURE_URL, GITHUB_REPO, copy } from '@/lib/site'

export function HomePage() {
  return (
    <div className="mx-auto w-full max-w-3xl px-[clamp(1rem,4vw,2.5rem)]">
      <p className="text-ink-soft mt-8 text-xs tracking-wide sm:mt-10">
        {copy.status}
      </p>
      <h1 className="mt-6 max-w-xl text-2xl leading-snug font-medium tracking-tight sm:text-3xl">
        {copy.line}
      </h1>
      <p className="text-ink-soft mt-4 max-w-xl text-base leading-relaxed">
        {copy.lede}
      </p>
      <p className="mt-8 flex flex-wrap gap-x-6 gap-y-2 text-sm">
        <a
          href={GITHUB_REPO}
          className="text-accent hover:underline"
          rel="noreferrer"
          target="_blank"
        >
          {copy.sourceCta}
        </a>
        <a
          href={ARCHITECTURE_URL}
          className="text-accent hover:underline"
          rel="noreferrer"
          target="_blank"
        >
          {copy.architectureCta}
        </a>
      </p>

      <div className="mt-16 sm:mt-20">
        <Journal />
      </div>
      <div className="mt-16 sm:mt-20">
        <Machine />
      </div>
      <p className="text-ink-soft mt-16 mb-4 max-w-xl text-sm leading-relaxed sm:mt-20">
        {copy.sourceBody}
      </p>
    </div>
  )
}
