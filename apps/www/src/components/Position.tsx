const cols = ['', 'urspace', 'Mesh VPN', 'Tunnel service', 'Port forward']

const rows: [string, string, string, string, string][] = [
  ['Guest setup', 'A browser', 'Install client, join network', 'None', 'None'],
  ['Host exposure', 'Loopback only, outbound', 'Whole device on the mesh', 'Public URL', 'Public port'],
  ['Who can reach it', 'Holders of a rotating invitation', 'Anyone on the network', 'Anyone with the URL', 'Anyone on the internet'],
  ['Who can read app traffic', 'Only host and guest', 'Only the peers', 'The provider, at its edge', 'Anyone on the path, unless the app adds TLS'],
]

export function Position() {
  return (
    <section id="compare" className="border-t border-[var(--line)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-10 lg:grid-cols-12">
          <div className="lg:col-span-4">
            <p className="eyebrow">Where it sits</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              A door with one key.
            </h2>
            <p className="mt-5 max-w-sm text-[15.5px] leading-relaxed text-[var(--ink-2)]">
              urspace admits an invited browser to one app on your machine.
              Traffic is encrypted end to end — only host and guest can read
              it.
            </p>
          </div>

          <div className="-mx-5 overflow-x-auto px-5 sm:mx-0 sm:px-0 lg:col-span-8">
            <table className="compare w-full min-w-[40rem] border-collapse text-left text-[13.5px]">
              <thead>
                <tr>
                  {cols.map((c, i) => (
                    <th
                      key={i}
                      scope="col"
                      className={`border-b border-[var(--line-strong)] pb-3 pr-4 font-medium ${
                        i === 1 ? 'text-[var(--ink)]' : 'text-[var(--ink-3)]'
                      } ${i === 0 ? 'w-[9rem]' : ''}`}
                    >
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map(([label, ...cells]) => (
                  <tr key={label} className="border-b border-[var(--line)]">
                    <th scope="row" className="py-4 pr-4 font-medium text-[var(--ink-2)]">
                      {label}
                    </th>
                    {cells.map((cell, i) => (
                      <td
                        key={i}
                        className={`py-4 pr-4 leading-snug ${
                          i === 0 ? 'font-medium text-[var(--ink)]' : 'text-[var(--ink-2)]'
                        }`}
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  )
}
