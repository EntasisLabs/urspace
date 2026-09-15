import { BrandMarkOnDark } from './BrandMark'

const links = [
  { href: 'https://github.com/EntasisLabs/urspace', label: 'GitHub' },
  { href: 'https://github.com/EntasisLabs/urspace/blob/main/docs/cli.md', label: 'CLI reference' },
  { href: 'https://github.com/EntasisLabs/urspace/blob/main/docs/session-grants.md', label: 'Protocol' },
  { href: 'https://github.com/EntasisLabs/urspace/releases', label: 'Releases' },
]

export function Footer() {
  return (
    <footer className="border-t border-[var(--dark-line)] bg-[var(--dark)] text-[var(--dark-muted)]">
      <div className="wrap flex flex-col gap-8 py-10 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-3">
          <BrandMarkOnDark className="h-5 w-5" />
          <span className="text-[13px]">A private space between peers.</span>
        </div>

        <nav className="flex flex-wrap gap-x-6 gap-y-2 text-[13px]" aria-label="Footer">
          {links.map((l) => (
            <a
              key={l.label}
              href={l.href}
              target="_blank"
              rel="noreferrer"
              className="transition-colors hover:text-white"
            >
              {l.label}
            </a>
          ))}
        </nav>

        <p className="font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-faint)]">
          Entasis Labs · MIT / Apache-2.0
        </p>
      </div>
    </footer>
  )
}
