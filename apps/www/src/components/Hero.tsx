import { Terminal } from './Terminal'

export function Hero() {
  return (
    <section id="top" className="relative overflow-hidden">
      <div className="rule-grid pointer-events-none absolute inset-0" aria-hidden="true" />

      <div className="wrap relative pb-16 pt-16 sm:pt-24 lg:pb-24 lg:pt-28">
        <div className="grid gap-14 lg:grid-cols-12 lg:items-center lg:gap-10">
          <div className="min-w-0 lg:col-span-6">
            <p className="eyebrow">Private local sharing</p>

            <h1 className="display mt-5 text-[clamp(2.35rem,6.4vw,4.4rem)] font-semibold leading-[1.02] text-[var(--ink)]">
              Your app stays on your machine.
              <br />
              <span className="text-[var(--ink-3)]">Your guest opens a link.</span>
            </h1>

            <p className="mt-7 max-w-[34rem] text-[17px] leading-[1.6] text-[var(--ink-2)] sm:text-lg">
              urspace gives the web app running on your machine a private,
              encrypted path to one invited browser. One command on your side.
              One link on theirs.
            </p>

            <div className="mt-9 flex flex-wrap items-center gap-3">
              <a
                href="#install"
                className="inline-flex h-11 items-center rounded-md bg-[var(--ink)] px-5 text-[14px] font-medium text-white transition-[background-color,transform] duration-150 hover:bg-[var(--dark-3)] active:scale-[0.99]"
              >
                Install the CLI
              </a>
              <a
                href="https://github.com/EntasisLabs/urspace/blob/main/docs/cli.md"
                target="_blank"
                rel="noreferrer"
                className="inline-flex h-11 items-center rounded-md border border-[var(--line-strong)] px-5 text-[14px] font-medium text-[var(--ink)] transition-colors duration-150 hover:border-[var(--ink)]"
              >
                Read the docs
              </a>
            </div>

            <ul className="mt-9 flex flex-wrap gap-x-5 gap-y-2 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.06em] text-[var(--ink-3)]">
              <li>Open source</li>
              <li>Written in Rust</li>
              <li>Transport by Iroh</li>
              <li>MIT / Apache-2.0</li>
            </ul>
          </div>

          <div className="min-w-0 lg:col-span-6">
            <Terminal title="~ — urspace" status="sharing">
              <span className="p">$</span> urspace serve localhost:8787 --ttl 10m --max-sessions 1 --short
              {'\n\n'}
              <span className="k">Urspace is serving app at http://127.0.0.1:8787</span>
              {'\n'}
              <span className="m">Only an encrypted link envelope was uploaded; application traffic travels over Iroh.</span>
              {'\n\n'}
              <span className="k">Share URL (treat it as a secret):</span>
              {'\n'}
              <span className="w">https://u.urspace.online/#s1=</span>
              <span className="f">••••••••••••••••••••••••••••••</span>
              {'\n\n'}
              <span className="m">Accepts new browser sessions for:</span> 10m
              {'\n'}
              <span className="m">Maximum admitted browser sessions:</span> 1
              {'\n'}
              <span className="m">Commands:</span> invite | rotate | raw | sessions | kick &lt;session&gt; | kick all
              {'\n'}
              <span className="m">Press Ctrl+C to stop sharing.</span>
              {'\n\n'}
              <span className="p">&gt;</span> <span className="caret" />
            </Terminal>
          </div>
        </div>

      </div>
    </section>
  )
}
