import { useEffect, useState } from 'react'
import { BrandWordmark } from './BrandMark'

const links = [
  { href: '#how', label: 'How' },
  { href: '#control', label: 'Control' },
  { href: '#modes', label: 'Modes' },
  { href: '#devices', label: 'Devices' },
  {
    href: 'https://github.com/EntasisLabs/urspace',
    label: 'GitHub',
    external: true,
  },
]

export function Nav() {
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!open) return
    const close = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    window.addEventListener('keydown', close)
    return () => window.removeEventListener('keydown', close)
  }, [open])

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
              className="hidden px-2.5 py-1.5 text-[13.5px] font-medium text-[var(--ink-2)] transition-colors hover:text-[var(--ink)] md:block"
            >
              {link.label}
            </a>
          ))}
          <a
            href="#install"
            className="ml-1 inline-flex h-9 items-center rounded-md bg-[var(--ink)] px-3.5 text-[13px] font-medium text-white transition-colors hover:bg-[var(--dark-3)] md:ml-3"
          >
            Install
          </a>
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            aria-controls="mobile-menu"
            aria-label={open ? 'Close menu' : 'Open menu'}
            className="ml-1 inline-flex h-9 w-9 items-center justify-center rounded-md text-[var(--ink)] transition-colors hover:bg-[var(--paper-2)] md:hidden"
          >
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true">
              {open ? (
                <path d="M4 4l10 10M14 4L4 14" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              ) : (
                <path d="M2 5h14M2 9h14M2 13h14" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              )}
            </svg>
          </button>
        </nav>
      </div>

      {open ? (
        <div id="mobile-menu" className="border-t border-[var(--line)] bg-[var(--paper)] md:hidden">
          <nav className="wrap flex flex-col py-2" aria-label="Mobile">
            {links.map((link) => (
              <a
                key={link.label}
                href={link.href}
                onClick={() => setOpen(false)}
                {...(link.external ? { target: '_blank', rel: 'noreferrer' } : {})}
                className="flex items-center justify-between border-b border-[var(--line)] py-3.5 text-[15px] font-medium text-[var(--ink)] last:border-b-0"
              >
                {link.label}
                {link.external ? (
                  <span className="font-[family-name:var(--font-mono)] text-[11px] text-[var(--ink-3)]">↗</span>
                ) : null}
              </a>
            ))}
          </nav>
        </div>
      ) : null}
    </header>
  )
}
