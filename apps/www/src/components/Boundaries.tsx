const boundaries = [
  {
    n: '01',
    title: 'Loopback, and only loopback.',
    body: 'The host only forwards to 127.0.0.1, ::1, or localhost. It refuses any other address, so it cannot be pointed at your LAN or the internet.',
  },
  {
    n: '02',
    title: 'The invitation is the key.',
    body: 'Fresh and random on every share. It expires on a timer you set, admits a fixed number of browsers, and can be replaced at any moment.',
  },
  {
    n: '03',
    title: 'Secrets stay off the disk.',
    body: 'Invitations and guest keys are never written to a log or a journal. What remains are limits and who you already admitted.',
  },
]

export function Boundaries() {
  return (
    <section id="boundaries" className="border-t border-[var(--line)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-10 lg:grid-cols-12">
          <div className="lg:col-span-4">
            <p className="eyebrow">Boundaries</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              Narrow on purpose.
            </h2>
            <p className="mt-5 max-w-sm text-[15.5px] leading-relaxed text-[var(--ink-2)]">
              What does the host expose, and to whom? Traffic is encrypted end
              to end. Only host and guest can read it.
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
