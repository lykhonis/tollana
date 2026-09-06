import { ExternalLink, Github } from 'pixelarticons/react'
import { brand } from '@/lib/mark'

type IconName = 'mark' | 'github' | 'external'

function Mark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="1em"
      height="1em"
      fill="currentColor"
      className={className}
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path d={brand.markPath} />
    </svg>
  )
}

const icons = {
  github: Github,
  external: ExternalLink,
} as const

export function PixelIcon({
  name,
  className,
}: {
  name: IconName
  className?: string
}) {
  if (name === 'mark') {
    return <Mark className={className} />
  }
  const Icon = icons[name]
  return <Icon className={className} width="1em" height="1em" />
}
