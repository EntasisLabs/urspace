const links = [
  { href: '#how', label: 'How' },
  { href: '#modes', label: 'Modes' },
  { href: '#install', label: 'Install' },
  {
    href: 'https://github.com/EntasisLabs/urspace',
    label: 'GitHub',
    external: true,
  },
]

export function Nav() {
  return (
    <header className="sticky top-0 z-40 border-b border-[var(--line)]/70 bg-[color-mix(in_srgb,var(--bg)_82%,transparent)] backdrop-blur-md">
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between gap-4 px-5 sm:px-8">
        <a
          href="#top"
          className="flex items-center gap-2.5 font-[family-name:var(--font-sans)] text-[15px] font-semibold tracking-tight text-[#0c0e10]"
        >
          <span
            className="inline-flex h-6 w-6 items-center justify-center bg-[#0c0e10]"
            aria-hidden
          >
            <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none">
              <path
                d="M2.5 11.5 8 3.5l5.5 8"
                stroke="#f4f6f7"
                strokeWidth="1.6"
                strokeLinejoin="round"
              />
              <circle cx="12.5" cy="4" r="1.3" fill="#2a8f78" />
            </svg>
          </span>
          urspace
        </a>
        <nav className="flex items-center gap-0.5 sm:gap-2" aria-label="Primary">
          {links.map((link) => (
            <a
              key={link.label}
              href={link.href}
              {...(link.external
                ? { target: '_blank', rel: 'noreferrer' }
                : {})}
              className="px-2 py-1.5 font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-[0.14em] text-[#5a6570] transition-colors hover:text-[#0c0e10] sm:px-2.5 sm:text-[11px]"
            >
              {link.label}
            </a>
          ))}
        </nav>
      </div>
    </header>
  )
}
