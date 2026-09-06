import { PixelIcon } from '@/components/PixelIcon'
import { ARCHITECTURE_URL, GITHUB_REPO, copy } from '@/lib/site'

export function HomePage() {
  return (
    <>
      <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] pt-10 pb-12 sm:pt-14 sm:pb-16 md:pt-20 md:pb-20">
        <h1 className="max-w-2xl text-2xl leading-tight font-medium tracking-tight sm:text-3xl md:text-4xl">
          {copy.line}
        </h1>
        <p className="text-ink-soft mt-4 max-w-xl text-base leading-relaxed sm:mt-5 sm:text-lg">
          {copy.lede}
        </p>
        <ul className="mt-6 flex flex-wrap gap-2 sm:mt-8">
          <li>
            <a
              href={GITHUB_REPO}
              className="border-hairline hover:bg-canvas-2 inline-flex min-h-11 items-center gap-2 border px-3 py-2 text-sm"
              rel="noreferrer"
              target="_blank"
            >
              <PixelIcon name="github" className="size-6" />
              {copy.sourceCta}
            </a>
          </li>
          <li>
            <a
              href={ARCHITECTURE_URL}
              className="border-hairline hover:bg-canvas-2 inline-flex min-h-11 items-center gap-2 border px-3 py-2 text-sm"
              rel="noreferrer"
              target="_blank"
            >
              <PixelIcon name="external" className="size-6" />
              {copy.architectureCta}
            </a>
          </li>
        </ul>
      </section>

      <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] pb-12 sm:pb-16 md:pb-20">
        <h2 className="text-ink-soft mb-3 font-mono text-xs tracking-widest uppercase sm:mb-4">
          {copy.qualitiesHeading}
        </h2>
        <ul className="border-hairline divide-hairline divide-y border">
          {copy.qualities.map((item) => (
            <li
              key={item.label}
              className="grid gap-1 px-3 py-3 sm:grid-cols-[10rem_1fr] sm:items-baseline sm:gap-6 sm:px-4 sm:py-4"
            >
              <span className="font-medium">{item.label}</span>
              <span className="text-ink-soft text-sm leading-relaxed sm:text-base">
                {item.line}
              </span>
            </li>
          ))}
        </ul>
      </section>

      <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] pb-12 sm:pb-16 md:pb-20">
        <h2 className="text-ink-soft mb-3 font-mono text-xs tracking-widest uppercase sm:mb-4">
          {copy.buildHeading}
        </h2>
        <ol className="border-hairline bg-canvas-2/40 space-y-0 border">
          {copy.build.map((item, index) => (
            <li
              key={item.label}
              className="border-hairline flex gap-4 border-b px-3 py-4 last:border-b-0 sm:px-4 sm:py-5"
            >
              <span className="text-accent font-mono text-xs tracking-widest">
                {String(index).padStart(2, '0')}
              </span>
              <div>
                <h3 className="font-medium">{item.label}</h3>
                <p className="text-ink-soft mt-1 max-w-2xl text-sm leading-relaxed sm:text-base">
                  {item.line}
                </p>
              </div>
            </li>
          ))}
        </ol>
      </section>

      <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] pb-16 sm:pb-20">
        <h2 className="text-ink-soft mb-3 font-mono text-xs tracking-widest uppercase sm:mb-4">
          {copy.sourceHeading}
        </h2>
        <p className="text-ink-soft max-w-xl text-base leading-relaxed">
          {copy.sourceBody}
        </p>
        <p className="text-ink-soft mt-4 text-sm">{copy.namedFor}</p>
      </section>
    </>
  )
}
