const steps = [
  {
    n: '01',
    who: 'Host',
    title: 'Run one command.',
    body: 'urspace confirms the local port is listening, mints a site identity and a random capability, and prints the invitation. Nothing about your app is uploaded.',
    code: 'urspace serve localhost:8787',
  },
  {
    n: '02',
    who: 'Guest',
    title: 'Open the link.',
    body: 'A small, generic bootstrap page loads once. The capability lives in the URL fragment, which browsers never send to a server. From there the page connects straight to your host.',
    code: 'https://u.urspace.online/#s1=…',
  },
  {
    n: '03',
    who: 'Both',
    title: 'Talk directly.',
    body: 'Requests travel end-to-end encrypted between host and browser over Iroh. Your app answers on loopback as if the guest were sitting at your machine.',
    code: 'browser ⇄ host · encrypted',
  },
]

export function HowItWorks() {
  return (
    <section id="how" className="border-t border-[var(--line)] bg-[var(--paper-2)]">
      <div className="wrap py-20 sm:py-28">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <p className="eyebrow">How it works</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              Three moves. No middle.
            </h2>
          </div>
          <p className="max-w-sm text-[14.5px] leading-relaxed text-[var(--ink-2)]">
            There is no account to create, no dashboard to visit, and no relay
            that reads your traffic.
          </p>
        </div>

        <ol className="mt-12 grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] md:grid-cols-3">
          {steps.map((s) => (
            <li key={s.n} className="flex flex-col bg-[var(--paper)] p-6 sm:p-7">
              <div className="flex items-baseline justify-between">
                <span className="num text-[11px] tracking-[0.12em] text-[var(--ink-3)]">{s.n}</span>
                <span className="eyebrow">{s.who}</span>
              </div>
              <h3 className="display mt-6 text-[20px] font-semibold leading-tight text-[var(--ink)]">
                {s.title}
              </h3>
              <p className="mt-3 flex-1 text-[14.5px] leading-relaxed text-[var(--ink-2)]">
                {s.body}
              </p>
              <code className="mt-6 block truncate border-t border-[var(--line)] pt-4 font-[family-name:var(--font-mono)] text-[12.5px] text-[var(--ink)]">
                {s.code}
              </code>
            </li>
          ))}
        </ol>
      </div>
    </section>
  )
}
