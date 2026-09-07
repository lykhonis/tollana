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
            key={item.cite}
            className="border-hairline grid grid-cols-[2.5rem_1fr] gap-x-4 border-t py-5 first:border-t-0 sm:grid-cols-[2.5rem_11rem_1fr] sm:gap-x-6"
          >
            <span className="text-ink-soft text-xs leading-6">
              {String(index).padStart(2, '0')}
            </span>
            <div className="min-w-0 sm:contents">
              <code className="text-accent block text-sm leading-6">
                {item.cite}
              </code>
              <div className="mt-2 sm:mt-0">
                <h3 className="leading-6 font-medium">{item.label}</h3>
                <p className="text-ink-soft mt-1 max-w-xl text-sm leading-relaxed">
                  {item.line}
                </p>
              </div>
            </div>
          </li>
        ))}
      </ol>
    </section>
  )
}
