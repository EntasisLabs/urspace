const rows = [
  {
    tool: 'Urspace',
    fit: 'Privately share one local web app with a person',
    who: 'Anyone with a valid invitation; browser only',
    setup: 'One urspace serve command',
    highlight: true,
  },
  {
    tool: 'Tailscale Serve',
    fit: 'Share inside an existing Tailscale network',
    who: 'Devices already on that network',
    setup: 'Tailscale on each device + access rules',
  },
  {
    tool: 'Tailscale Funnel / Cloudflare Tunnel',
    fit: 'Stable public hostname in front of a private service',
    who: 'The internet, or platform access rules',
    setup: 'Account, connector, DNS, platform config',
  },
]

export function Position() {
  return (
    <section className="border-t border-[var(--line)] bg-[var(--dark)] text-[var(--bg-elevated)]">
      <div className="mx-auto max-w-6xl px-5 py-20 sm:px-8 sm:py-28">
        <p className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.2em] text-[var(--signal)]">
          / not a VPN
        </p>
        <h2 className="mt-4 max-w-2xl text-3xl font-semibold tracking-tight sm:text-4xl">
          Built for invitation-shaped access.
        </h2>
        <p className="mt-4 max-w-2xl text-base leading-relaxed text-[var(--dark-muted)] sm:text-lg">
          Related tools start from networks and hostnames. Urspace starts from a
          temporary secret that admits a browser to one local origin.
        </p>

        <div className="mt-12 overflow-x-auto">
          <table className="w-full min-w-[40rem] border-collapse text-left text-sm">
            <thead>
              <tr className="border-b border-[var(--dark-line)] font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.14em] text-[var(--dark-muted)]">
                <th className="pb-4 pr-6 font-medium">Tool</th>
                <th className="pb-4 pr-6 font-medium">Best fit</th>
                <th className="pb-4 pr-6 font-medium">Who can open</th>
                <th className="pb-4 font-medium">Host setup</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr
                  key={row.tool}
                  className="border-b border-[var(--dark-line)] align-top"
                >
                  <td
                    className={`py-5 pr-6 font-medium ${
                      row.highlight ? 'text-[var(--signal)]' : ''
                    }`}
                  >
                    {row.tool}
                  </td>
                  <td className="py-5 pr-6 text-[var(--dark-muted)]">{row.fit}</td>
                  <td className="py-5 pr-6 text-[var(--dark-muted)]">{row.who}</td>
                  <td className="py-5 text-[var(--dark-muted)]">{row.setup}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  )
}
