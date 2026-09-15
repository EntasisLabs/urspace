import type { ReactNode } from 'react'

export function Terminal({
  title,
  status,
  children,
  className = '',
}: {
  title: string
  status?: string
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={`overflow-hidden rounded-[10px] border border-[var(--dark-line)] bg-[var(--dark)] shadow-[0_1px_0_rgba(255,255,255,0.04)_inset,0_24px_60px_-30px_rgba(15,24,39,0.55)] ${className}`}
    >
      <div className="flex h-10 items-center justify-between border-b border-[var(--dark-line)] px-4">
        <span className="font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-muted)]">
          {title}
        </span>
        {status ? (
          <span className="flex items-center gap-2 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-muted)]">
            <span className="live inline-block h-1.5 w-1.5 rounded-full bg-[var(--signal)]" />
            {status}
          </span>
        ) : null}
      </div>
      <pre className="term px-4 py-4 sm:px-5 sm:py-5">
        <code>{children}</code>
      </pre>
    </div>
  )
}
