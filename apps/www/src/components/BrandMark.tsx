import { bowlPath, brand } from '@/lib/mark'

export function BrandMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="1em"
      height="1em"
      className={className}
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
    >
      <circle
        cx="12"
        cy="12"
        r="8.9"
        fill="none"
        stroke={brand.navy}
        strokeWidth="0.8"
      />
      <circle
        cx="12"
        cy="12"
        r="7.5"
        fill="none"
        stroke={brand.ring}
        strokeWidth="2"
      />
      <path fill={brand.bowl} d={bowlPath} />
    </svg>
  )
}
