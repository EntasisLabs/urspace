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
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-5 sm:px-8">
        <a href="#top" className="font-[family-name:var(--font-sans)] text-[15px] font-semibold tracking-tight text-[var(--ink)]">
          urspace
        </a>
        <nav className="flex items-center gap-1 sm:gap-2" aria-label="Primary">
          {links.map((link) => (
            <a
              key={link.label}
              href={link.href}
              {...(link.external
                ? { target: '_blank', rel: 'noreferrer' }
                : {})}
              className="px-2.5 py-1.5 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.14em] text-[var(--muted)] transition-colors hover:text-[var(--ink)]"
            >
              {link.label}
            </a>
          ))}
        </nav>
      </div>
    </header>
  )
}
