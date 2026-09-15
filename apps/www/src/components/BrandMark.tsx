/**
 * Urspace brand mark — Brand Identity Variation 01 (Standard / Refined).
 * Paths traced from the sheet icon: black slab with top-right tab + gray plane.
 */
const BLACK =
  'M5.27 12.75 28.88 1.81h4.51v11.42l-6.91.19v45.31H5.27V12.75Z'
const GRAY = 'M34.64 21.97 58.73 12.75v49.44L34.64 55.85V21.97Z'

export function BrandMark({
  className = 'h-6 w-6',
  title = 'urspace',
}: {
  className?: string
  title?: string
}) {
  return (
    <svg
      className={className}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={title}
    >
      <title>{title}</title>
      <path d={BLACK} fill="#000000" />
      <path d={GRAY} fill="#5A5A5A" />
    </svg>
  )
}

export function BrandMarkOnDark({
  className = 'h-6 w-6',
  title = 'urspace',
}: {
  className?: string
  title?: string
}) {
  return (
    <svg
      className={className}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={title}
    >
      <title>{title}</title>
      <path d={BLACK} fill="#FFFFFF" />
      <path d={GRAY} fill="#B3B3B3" />
    </svg>
  )
}
