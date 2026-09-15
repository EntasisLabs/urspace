/**
 * Urspace brand mark.
 * Prefer the official PNG (`/brand/mark.png`) for UI; SVG paths are a traced fallback.
 */
export function BrandMark({
  className = 'h-6 w-6',
  title = 'urspace',
}: {
  className?: string
  title?: string
}) {
  return (
    <img
      src="/brand/mark.png"
      alt={title}
      className={`object-contain ${className}`}
      width={64}
      height={64}
      decoding="async"
    />
  )
}

export function BrandMarkOnDark({
  className = 'h-6 w-6',
  title = 'urspace',
}: {
  className?: string
  title?: string
}) {
  // Knockout version for dark surfaces — use official app-icon crop via SVG fallback colors
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
      <path
        d="M6.19 14.55 35.11 1.52h.25v14.02l-7.76.05v35.15H6.19V14.55Z"
        fill="#FFFFFF"
      />
      <path
        d="M36.12 24.25 57.81 15.38v47.1L36.12 54.44V24.25Z"
        fill="#9AA0A8"
      />
    </svg>
  )
}

export function BrandWordmark({
  className = 'h-8',
  title = 'urspace',
  onDark = false,
}: {
  className?: string
  title?: string
  onDark?: boolean
}) {
  return (
    <img
      src={onDark ? '/brand/wordmark-on-dark-sm.png' : '/brand/wordmark-sm.png'}
      alt={title}
      className={`w-auto max-w-none object-contain object-left ${className}`}
      width={720}
      height={240}
      decoding="async"
    />
  )
}
