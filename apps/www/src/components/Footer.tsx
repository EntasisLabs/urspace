import { BrandMark } from './BrandMark'

export function Footer() {
  return (
    <footer className="border-t border-[var(--line)]">
      <div className="mx-auto flex max-w-6xl flex-col gap-6 px-5 py-10 sm:flex-row sm:items-end sm:justify-between sm:px-8">
        <div>
          <div className="flex items-center gap-2.5">
            <BrandMark className="h-5 w-5" />
            <p className="text-lg font-semibold tracking-tight text-[var(--ink)]">
              urspace
            </p>
          </div>
          <p className="mt-2 max-w-sm font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.14em] text-[var(--muted)]">
            A private space between peers.
          </p>
          <p className="mt-2 max-w-sm text-sm leading-relaxed text-[var(--muted)]">
            Private encrypted access to a local origin. Apache-2.0 or MIT.
          </p>
        </div>
        <div className="flex flex-wrap gap-x-5 gap-y-2 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.14em] text-[var(--muted)]">
          <a
            href="https://github.com/EntasisLabs/urspace"
            target="_blank"
            rel="noreferrer"
            className="hover:text-[var(--ink)]"
          >
            GitHub
          </a>
          <a
            href="https://github.com/EntasisLabs/urspace/blob/main/docs/cli.md"
            target="_blank"
            rel="noreferrer"
            className="hover:text-[var(--ink)]"
          >
            CLI
          </a>
          <a
            href="https://github.com/EntasisLabs/urspace/blob/main/SECURITY.md"
            target="_blank"
            rel="noreferrer"
            className="hover:text-[var(--ink)]"
          >
            Security
          </a>
          <a
            href="https://github.com/EntasisLabs/urspace/blob/main/skills/urspace/SKILL.md"
            target="_blank"
            rel="noreferrer"
            className="hover:text-[var(--ink)]"
          >
            Agent skill
          </a>
        </div>
      </div>
    </footer>
  )
}
