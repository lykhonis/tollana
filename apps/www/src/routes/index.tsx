import { createFileRoute } from '@tanstack/react-router'
import { HomePage } from '@/pages/home'
import { pageHead } from '@/lib/seo'

export const Route = createFileRoute('/')({
  head: () => {
    const seo = pageHead()
    return { meta: seo.meta, links: seo.links }
  },
  component: HomePage,
})
