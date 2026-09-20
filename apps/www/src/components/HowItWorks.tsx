const steps = [
  {
    n: '01',
    who: 'Host',
    title: 'Run one command.',
    body: 'Your app stays on your machine.',
  },
  {
    n: '02',
    who: 'Guest',
    title: 'Open the link.',
    body: 'They join in a browser. No account.',
  },
  {
    n: '03',
    who: 'Both',
    title: 'Talk privately.',
    body: 'Encrypted end to end. Only host and guest can read it.',
  },
]

export function HowItWorks() {
  return (
    <section id="how" className="border-t border-[var(--line)] bg-[var(--paper-2)]">
      <div className="wrap py-20 sm:py-28">
        <p className="eyebrow">How it works</p>
        <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
          Three moves.
        </h2>

        <ol className="mt-12 grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] md:grid-cols-3">
          {steps.map((s) => (
            <li key={s.n} className="flex min-w-0 flex-col bg-[var(--paper)] p-6 sm:p-7">
              <div className="flex items-baseline justify-between">
                <span className="num text-[11px] tracking-[0.12em] text-[var(--ink-3)]">{s.n}</span>
                <span className="eyebrow">{s.who}</span>
              </div>
              <h3 className="display mt-6 text-[20px] font-semibold leading-tight text-[var(--ink)]">
                {s.title}
              </h3>
              <p className="mt-3 text-[14.5px] leading-relaxed text-[var(--ink-2)]">{s.body}</p>
            </li>
          ))}
        </ol>
      </div>
    </section>
  )
}
