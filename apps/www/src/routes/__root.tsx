/// <reference types="vite/client" />
import * as React from 'react'
import { HeadContent, Scripts, createRootRoute } from '@tanstack/react-router'
import { DefaultCatchBoundary } from '@/components/DefaultCatchBoundary'
import { Footer } from '@/components/Footer'
import { Header } from '@/components/Header'
import { NotFound } from '@/components/NotFound'
import { softwareJsonLd } from '@/lib/seo'
import appCss from '@/styles/app.css?url'

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { name: 'theme-color', content: '#F6F3EC' },
    ],
    links: [
      { rel: 'stylesheet', href: appCss },
      { rel: 'icon', href: '/favicon.svg', type: 'image/svg+xml' },
      { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' },
    ],
  }),
  errorComponent: DefaultCatchBoundary,
  notFoundComponent: () => <NotFound />,
  shellComponent: RootDocument,
})

function RootDocument({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{
            __html: JSON.stringify(softwareJsonLd()),
          }}
        />
      </head>
      <body className="bg-canvas text-ink min-h-dvh antialiased">
        <div className="flex min-h-dvh flex-col">
          <Header />
          <main className="flex flex-1 flex-col">{children}</main>
          <Footer />
        </div>
        <Scripts />
      </body>
    </html>
  )
}
