const boundaries = [
  {
    n: '01',
    title: 'Loopback, and only loopback.',
    body: 'The host forwards to 127.0.0.1, ::1, or localhost. It refuses credentials, paths, and any other address, so it cannot be pointed at your LAN or the internet.',
  },
  {
    n: '02',
    title: 'The invitation is the key.',
    body: 'Fresh and random on every share. It expires on a timer you set, admits a fixed number of browsers, and can be rotated at any moment. Whoever holds it may knock.',
  },
  {
    n: '03',
    title: 'Secrets stay off the disk.',
    body: 'Invitations, capabilities, and browser keys are never written to a log or a journal. What persists is hashes, limits, and revocation state.',
  },
]

export function Principles() {
  return (
    <section id="why" className="border-t border-[var(--line)] bg-[var(--paper-2)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-10 lg:grid-cols-12">
          <div className="lg:col-span-4">
            <p className="eyebrow">Boundaries</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              Narrow on purpose.
            </h2>
            <p className="mt-5 max-w-sm text-[15.5px] leading-relaxed text-[var(--ink-2)]">
              Every decision starts from one question: what does the host
              expose, and to whom?
            </p>
          </div>

          <ol className="grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] lg:col-span-8">
            {boundaries.map((p) => (
              <li
                key={p.n}
                className="grid gap-3 bg-[var(--paper)] p-6 sm:grid-cols-[3rem_14rem_1fr] sm:gap-6 sm:p-7"
              >
                <span className="num text-[11px] tracking-[0.12em] text-[var(--ink-3)] sm:pt-1">
                  {p.n}
                </span>
                <h3 className="display text-[19px] font-semibold leading-tight text-[var(--ink)]">
                  {p.title}
                </h3>
                <p className="text-[14.5px] leading-relaxed text-[var(--ink-2)]">{p.body}</p>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  )
}
