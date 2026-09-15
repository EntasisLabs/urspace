/**
 * Urspace brand mark — two perspective slabs forming a private space.
 * Variation 01 (Standard / Refined).
 */
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
      <path d="M14 54V16l12-6v6h5v32L14 54Z" fill="#0C0E10" />
      <path d="M31 48V22l16 6v24L31 48Z" fill="#6B7076" />
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
      <path d="M14 54V16l12-6v6h5v32L14 54Z" fill="#F4F6F7" />
      <path d="M31 48V22l16 6v24L31 48Z" fill="#9AA0A6" />
    </svg>
  )
}
