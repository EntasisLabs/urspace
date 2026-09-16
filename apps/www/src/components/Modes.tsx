const modes = [
  {
    cmd: 'urspace serve localhost:8787',
    title: 'Share for now',
    body: 'Foreground. One hour and four browsers by default. Lives as long as the terminal does.',
  },
  {
    cmd: 'urspace serve localhost:8787 --short',
    title: 'Make the link short',
    body: 'Publishes only an encrypted, signed envelope. The seed rides in the fragment, so the short-link service cannot read what it stores.',
  },
  {
    cmd: 'urspace service install localhost:8787 --name boxclub',
    title: 'Keep it running',
    body: 'A per-user launchd or systemd service. Starts at login, survives restarts, and remembers who you already admitted.',
  },
  {
    cmd: 'urspace service invite boxclub --for "Alice / laptop"',
    title: 'Invite by name',
    body: 'Label each invitation and default it to one seat. Add --tcp to enroll a device instead of a browser.',
  },
]

export function Modes() {
  return (
    <section id="modes" className="border-t border-[var(--line)]">
      <div className="wrap py-20 sm:py-28">
        <p className="eyebrow">Modes</p>
        <h2 className="display mt-4 max-w-xl text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
          Start with a demo. Grow into a service.
        </h2>

        <ul className="mt-12 grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] sm:grid-cols-2">
          {modes.map((m) => (
            <li key={m.cmd} className="min-w-0 bg-[var(--paper)] p-6 sm:p-7">
              <code className="block whitespace-pre-wrap [overflow-wrap:anywhere] font-[family-name:var(--font-mono)] text-[12.5px] leading-relaxed text-[var(--ink)]">
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
