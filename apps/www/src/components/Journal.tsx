import { copy } from '@/lib/site'

export function Journal() {
  return (
    <section aria-labelledby="journal-heading">
      <h2 id="journal-heading" className="text-accent text-sm">
        {copy.journalHeading}
      </h2>
      <ol className="mt-6">
        {copy.journal.map((item, index) => (
          <li
            key={item.label}
            className="border-hairline grid grid-cols-[2.5rem_1fr] gap-x-4 border-t py-5 first:border-t-0 sm:gap-x-6"
          >
            <span className="text-ink-soft text-xs leading-6">
              {String(index).padStart(2, '0')}
            </span>
            <div>
              <h3 className="leading-6 font-medium">{item.label}</h3>
              <p className="text-ink-soft mt-1 max-w-xl text-sm leading-relaxed">
                {item.line}
              </p>
            </div>
          </li>
        ))}
      </ol>
    </section>
  )
}
