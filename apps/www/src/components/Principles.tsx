const principles = [
  {
    n: '01',
    title: 'Loopback only.',
    body: 'The host forwards to 127.0.0.1, ::1, or localhost and nothing else. It checks the port is listening before it mints an identity. It cannot be turned into an open proxy for your LAN or the internet.',
  },
  {
    n: '02',
    title: 'The invitation is the key.',
    body: 'A fresh random capability every time you share. It expires, it caps sessions, and you can rotate it without restarting. Whoever holds it may knock. Nobody else can.',
  },
  {
    n: '03',
    title: 'Nothing sees your app but the browser.',
    body: 'The bootstrap page is generic and immutable. Once loaded, requests travel encrypted between host and browser over Iroh. The bootstrap origin never carries application traffic.',
  },
  {
    n: '04',
    title: 'Stop, and it is gone.',
    body: 'A foreground share ends with Ctrl+C. A named service remembers who you admitted, keeps kicked browsers kicked, and never writes an invitation into a log.',
  },
]

export function Principles() {
  return (
    <section id="why" className="border-t border-[var(--line)] bg-[var(--paper-2)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-10 lg:grid-cols-12">
          <div className="lg:col-span-4">
            <p className="eyebrow">Why it holds</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              Built like infrastructure. Used like a link.
            </h2>
            <p className="mt-5 max-w-sm text-[15.5px] leading-relaxed text-[var(--ink-2)]">
              Every decision starts from the same question: what does the host
              expose, and who can reach it? The answers are narrow on purpose.
            </p>
          </div>

          <ol className="grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] sm:grid-cols-2 lg:col-span-8">
            {principles.map((p) => (
              <li key={p.n} className="bg-[var(--paper)] p-6 sm:p-7">
                <span className="num text-[11px] tracking-[0.12em] text-[var(--ink-3)]">{p.n}</span>
                <h3 className="display mt-4 text-[19px] font-semibold leading-tight text-[var(--ink)]">
                  {p.title}
                </h3>
                <p className="mt-3 text-[14.5px] leading-relaxed text-[var(--ink-2)]">{p.body}</p>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  )
}
