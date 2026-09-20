import { Terminal } from './Terminal'

const commands = [
  ['sessions', 'See who is in.'],
  ['kick <session>', 'Remove a guest.'],
  ['invite', 'Hand out a new link.'],
]

export function Control() {
  return (
    <section id="control" className="border-t border-[var(--line)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-12 lg:grid-cols-12 lg:items-start lg:gap-10">
          <div className="min-w-0 lg:col-span-5">
            <p className="eyebrow">Control</p>
            <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
              You stay in the room.
            </h2>
            <p className="mt-5 max-w-md text-[15.5px] leading-relaxed text-[var(--ink-2)]">
              See who is inside. Remove them. Hand out a new link. The share
              stays up.
            </p>

            <dl className="mt-9 divide-y divide-[var(--line)] border-y border-[var(--line)]">
              {commands.map(([cmd, desc]) => (
                <div key={cmd} className="grid grid-cols-[9.5rem_1fr] gap-4 py-3.5">
                  <dt className="font-[family-name:var(--font-mono)] text-[13px] text-[var(--ink)]">
                    {cmd}
                  </dt>
                  <dd className="text-[14px] leading-snug text-[var(--ink-2)]">{desc}</dd>
                </div>
              ))}
            </dl>
          </div>

          <div className="min-w-0 lg:col-span-7">
            <Terminal title="~ — urspace" status="sharing">
              <span className="p">&gt;</span> sessions
              {'\n'}
              <span className="k">session-k3v8  connected</span>
              {'\n'}
              <span className="k">session-p1qm  disconnected</span>
              {'\n\n'}
              <span className="p">&gt;</span> kick session-p1qm
              {'\n'}
              <span className="k">Removed session-p1qm.</span>
              {'\n\n'}
              <span className="p">&gt;</span> sessions
              {'\n'}
              <span className="k">session-k3v8  connected</span>
              {'\n\n'}
              <span className="p">&gt;</span> <span className="caret" />
            </Terminal>
          </div>
        </div>
      </div>
    </section>
  )
}
