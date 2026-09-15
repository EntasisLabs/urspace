const modes = [
  {
    label: 'serve',
    title: 'Foreground share',
    body: 'Temporary private access to an HTTP app already on loopback. Stop the process and the site goes away.',
    command: 'urspace serve localhost:8787 --short',
  },
  {
    label: 'static',
    title: 'Directory share',
    body: 'Read-only share of a folder, confined to that root, with an explicit entry page.',
    command: 'urspace static ./public --entry-path /index.html',
  },
  {
    label: 'service',
    title: 'Named persistence',
    body: 'Install a named site that survives terminal close, failures, and login. Rotate invites; keep admitted sessions.',
    command: 'urspace service install localhost:8787 --name app',
  },
  {
    label: 'connect',
    title: 'Managed device',
    body: 'Enroll another machine and mount the private app on local loopback—raw TCP when you ask for it.',
    command: 'urspace connect app localhost:9090 --invite-stdin',
  },
]

export function Modes() {
  return (
    <section id="modes" className="border-t border-[var(--line)]">
      <div className="mx-auto max-w-6xl px-5 py-20 sm:px-8 sm:py-28">
        <p className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.2em] text-[var(--muted)]">
          / operating modes
        </p>
        <h2 className="mt-4 max-w-xl text-3xl font-semibold tracking-tight text-[var(--ink)] sm:text-4xl">
          One boundary. Four ways to run it.
        </h2>
        <p className="mt-4 max-w-2xl text-base leading-relaxed text-[var(--muted)] sm:text-lg">
          Always an explicit directory or loopback upstream—never a proxy to the
          LAN or the public internet.
        </p>

        <div className="mt-14 divide-y divide-[var(--line)] border-y border-[var(--line)]">
          {modes.map((mode) => (
            <article
              key={mode.label}
              className="grid gap-4 py-8 md:grid-cols-[7rem_1fr_minmax(0,22rem)] md:items-start md:gap-8"
            >
              <p className="font-[family-name:var(--font-mono)] text-[12px] uppercase tracking-[0.16em] text-[var(--accent)]">
                {mode.label}
              </p>
              <div>
                <h3 className="text-xl font-semibold tracking-tight text-[var(--ink)]">
                  {mode.title}
                </h3>
                <p className="mt-2 max-w-xl text-[15px] leading-relaxed text-[var(--muted)]">
                  {mode.body}
                </p>
              </div>
              <pre className="overflow-x-auto bg-[var(--dark)] px-4 py-3 font-[family-name:var(--font-mono)] text-[12px] leading-relaxed text-[#d7dde2] md:justify-self-end md:self-center">
                <code>{mode.command}</code>
              </pre>
            </article>
          ))}
        </div>
      </div>
    </section>
  )
}
