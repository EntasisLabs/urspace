import { BrandWordmark } from './BrandMark'

const links = [
  { href: '#how', label: 'How' },
  { href: '#control', label: 'Control' },
  { href: '#modes', label: 'Modes' },
  {
    href: 'https://github.com/EntasisLabs/urspace',
    label: 'GitHub',
    external: true,
  },
]

export function Nav() {
  return (
    <header className="sticky top-0 z-40 border-b border-[var(--line)] bg-[color-mix(in_srgb,var(--paper)_86%,transparent)] backdrop-blur-md">
      <div className="wrap flex h-16 items-center justify-between gap-6">
        <a href="#top" className="flex items-center" aria-label="urspace home">
          <BrandWordmark className="h-7 sm:h-8" />
        </a>

        <nav className="flex items-center gap-1 sm:gap-2" aria-label="Primary">
          {links.map((link) => (
            <a
              key={link.label}
              href={link.href}
              {...(link.external ? { target: '_blank', rel: 'noreferrer' } : {})}
              className="hidden px-2.5 py-1.5 text-[13.5px] font-medium text-[var(--ink-2)] transition-colors hover:text-[var(--ink)] sm:block"
            >
              {link.label}
            </a>
          ))}
          <a
            href="#install"
            className="ml-1 inline-flex h-9 items-center rounded-md bg-[var(--ink)] px-3.5 text-[13px] font-medium text-white transition-colors hover:bg-[var(--dark-3)] sm:ml-3"
          >
            Install
          </a>
        </nav>
      </div>
    </header>
  )
}
