import { Terminal } from './Terminal'

const points = [
  ['Any TCP', 'HTTP, WebSockets, SSH, a database. Bytes are carried, not parsed.'],
  ['Bound to loopback', 'The mount listens on 127.0.0.1 of the device. Nothing else on the network can see it.'],
  ['Not a bearer token', 'The host challenges the device key on every reconnect. A kick lands on a device exactly as it does on a browser.'],
]

export function Devices() {
  return (
    <section id="devices" className="border-t border-[var(--line)] bg-[var(--paper-2)]">
      <div className="wrap py-20 sm:py-28">
        <div className="max-w-2xl">
          <p className="eyebrow">Devices</p>
          <h2 className="display mt-4 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-[var(--ink)]">
            Two CLIs. One private wire.
          </h2>
          <p className="mt-5 text-[15.5px] leading-relaxed text-[var(--ink-2)]">
            When the guest is a machine you trust, skip the browser. The host
            issues a device invitation; the device mounts the service on a local
            port. Anything that speaks TCP talks to localhost, and the bytes
            travel over the same encrypted connection.
          </p>
        </div>

        <div className="mt-12 grid gap-4 lg:grid-cols-2">
          <div className="min-w-0">
            <Terminal title="host — boxclub" status="service">
              <span className="p">$</span> urspace service invite boxclub --for "Alice / work laptop" --tcp
              {'\n\n'}
              <span className="k">Share URL (treat it as a secret):</span>
              {'\n'}
              <span className="w">https://</span><span className="f">••••••</span><span className="w">.urspace.online/.urspace/open/#u4=</span><span className="f">••••••••••••</span>
              {'\n\n'}
              <span className="p">$</span> urspace service sessions boxclub
              {'\n'}
              <span className="k">Alice / work laptop  session-r8dn  connected</span>
            </Terminal>
          </div>

          <div className="min-w-0">
            <Terminal title="alice — work laptop" status="connected">
              <span className="p">$</span> urspace connect boxclub localhost:9090 --invite-stdin
              {'\n'}
              <span className="m">Paste the one-time enrollment invitation, then press Enter:</span>
              {'\n'}
              <span className="f">••••••••••••••••••••••••••••••••••••••••</span>
              {'\n\n'}
              <span className="k">urspace mounted `boxclub` at localhost:9090.</span>
              {'\n'}
              <span className="m">TCP traffic is carried over the encrypted Iroh connection.</span>
              {'\n\n'}
              <span className="p">$</span> psql -h localhost -p 9090 boxclub
              {'\n'}
              <span className="k">boxclub=&gt;</span> <span className="caret" />
            </Terminal>
          </div>
        </div>

        <dl className="mt-10 grid gap-px overflow-hidden rounded-[10px] border border-[var(--line)] bg-[var(--line)] sm:grid-cols-3">
          {points.map(([term, detail]) => (
            <div key={term} className="min-w-0 bg-[var(--paper)] p-6 sm:p-7">
              <dt className="display text-[17px] font-semibold text-[var(--ink)]">{term}</dt>
              <dd className="mt-2 text-[14.5px] leading-relaxed text-[var(--ink-2)]">{detail}</dd>
            </div>
          ))}
        </dl>

        <p className="mt-6 text-[13px] leading-relaxed text-[var(--ink-3)]">
          Later, <code className="font-[family-name:var(--font-mono)] text-[var(--ink-2)]">urspace connect boxclub</code> restores
          the same mount with the saved device key. No invitation is needed
          twice.
        </p>
      </div>
    </section>
  )
}
