import { copy } from '@/lib/site'

export function HomePage() {
  return (
    <section className="mx-auto w-full max-w-5xl px-[clamp(1rem,4vw,3rem)] pt-10 pb-12 sm:pt-14 sm:pb-16 md:pt-20 md:pb-20">
      <h1 className="max-w-2xl text-2xl leading-tight font-medium tracking-tight sm:text-3xl md:text-4xl">
        {copy.siteName}
      </h1>
    </section>
  )
}
