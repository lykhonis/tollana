import { createFileRoute } from '@tanstack/react-router'
import { renderLlmsTxt } from '@/lib/llms-txt'

export const Route = createFileRoute('/llms.txt')({
  server: {
    handlers: {
      GET: async () => {
        return new Response(renderLlmsTxt(), {
          headers: {
            'content-type': 'text/plain; charset=utf-8',
            'cache-control': 'public, max-age=60, stale-while-revalidate=300',
          },
        })
      },
    },
  },
})
