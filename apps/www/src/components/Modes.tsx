const modes = [
  {
    cmd: 'urspace serve localhost:8787',
    title: 'Share for now',
    body: 'Foreground. A direct capability link, one hour, four browsers by default. Ctrl+C ends it.',
  },
  {
    cmd: 'urspace serve localhost:8787 --short',
    title: 'Share with a short link',
    body: 'Publishes only an encrypted, signed envelope. The seed stays in the fragment; the service cannot read it.',
  },
  {
    cmd: 'urspace service install localhost:8787 --name boxclub',
    title: 'Keep it running',
    body: 'A per-user launchd or systemd service. Survives restarts, remembers admissions, never logs a link.',
  },
  {
    cmd: 'urspace service invite boxclub --for "Alice / laptop"',
    title: 'Invite by name',
    body: 'Labelled, single-seat invitations over an authenticated loopback control connection. Kick by handle.',
  },
]

export function Modes() {
  return (
    <section id="modes" className="border-t border-[var(--line)]">
      <div className="wrap py-20 sm:py-28">
        <p className="eyebrow">Modes</p>
        <h2 className="display mt-4 max-w-xl text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
          One tool. From a five-minute demo to a service you forget about.
        </h2>

        <ul className="mt-12 grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] sm:grid-cols-2">
          {modes.map((m) => (
            <li key={m.cmd} className="group bg-[var(--paper)] p-6 sm:p-7">
              <code className="block overflow-x-auto whitespace-nowrap font-[family-name:var(--font-mono)] text-[12.5px] text-[var(--ink)]">
                <span className="text-[var(--ink-3)]">$ </span>
                {m.cmd}
              </code>
              <h3 className="display mt-6 text-[19px] font-semibold leading-tight text-[var(--ink)]">
                {m.title}
              </h3>
              <p className="mt-2.5 text-[14.5px] leading-relaxed text-[var(--ink-2)]">{m.body}</p>
            </li>
          ))}
        </ul>
      </div>
    </section>
  )
}
