import { useState, type ReactNode } from 'react'

const serveCmd = 'urspace serve localhost:8787'
const installThenServeCmd =
  "curl --proto '=https' --tlsv1.2 -fsSL https://github.com/EntasisLabs/urspace/releases/latest/download/install-urspace.sh | bash && ~/.local/bin/urspace serve localhost:8787"

export function Hero() {
  const [copied, setCopied] = useState(false)

  async function copyCommand() {
    const ok = await copyText(installThenServeCmd)
    if (!ok) return
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1600)
  }

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
              encrypted path to an invited browser. One command on your side.
              One link on theirs.
            </p>

            <div className="mt-9 flex flex-wrap items-center gap-3">
              <button
                type="button"
                onClick={copyCommand}
                aria-live="polite"
                className="inline-flex h-11 items-center rounded-md bg-[var(--ink)] px-5 text-[14px] font-medium text-white transition-[background-color,transform] duration-150 hover:bg-[var(--dark-3)] active:scale-[0.99]"
              >
                {copied ? 'Copied' : 'Copy install + serve'}
              </button>
              <a
                href="#install"
                className="inline-flex h-11 items-center rounded-md border border-[var(--line-strong)] px-5 text-[14px] font-medium text-[var(--ink)] transition-colors duration-150 hover:border-[var(--ink)]"
              >
                Install
              </a>
              <a
                href="https://github.com/EntasisLabs/urspace/blob/main/docs/cli.md"
                target="_blank"
                rel="noreferrer"
                className="inline-flex h-11 items-center rounded-md border border-[var(--line-strong)] px-5 text-[14px] font-medium text-[var(--ink)] transition-colors duration-150 hover:border-[var(--ink)]"
              >
                Docs
              </a>
            </div>

            <ul className="mt-9 flex flex-wrap items-center gap-x-3 gap-y-2 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.06em] text-[var(--ink-3)]">
              <li>Open source</li>
              <li aria-hidden="true">·</li>
              <li>No account for guests</li>
              <li aria-hidden="true">·</li>
              <li>Ends when you stop</li>
            </ul>
          </div>

          <div className="min-w-0 space-y-3 lg:col-span-6">
            <ProofBeat label="Run">
              <span className="p">$</span> {serveCmd}
            </ProofBeat>
            <ProofBeat label="Connected">
              <span className="k">
                <span className="text-[var(--signal)]">●</span> Private link ready
              </span>
              {'\n'}
              <span className="m">localhost:8787 · encrypted end to end</span>
            </ProofBeat>
            <ProofBeat label="Share" status="private url">
              <span className="w">https://u.urspace.online/</span>
              <span className="f">••••••••••••••••</span>
            </ProofBeat>
          </div>
        </div>
      </div>
    </section>
  )
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const input = document.createElement('textarea')
      input.value = text
      input.setAttribute('readonly', '')
      input.style.position = 'fixed'
      input.style.left = '-9999px'
      document.body.appendChild(input)
      input.select()
      const ok = document.execCommand('copy')
      document.body.removeChild(input)
      return ok
    } catch {
      return false
    }
  }
}

function ProofBeat({
  label,
  status,
  children,
}: {
  label: string
  status?: string
  children: ReactNode
}) {
  return (
    <div className="overflow-hidden rounded-[10px] border border-[var(--dark-line)] bg-[var(--dark)] shadow-[0_1px_0_rgba(255,255,255,0.04)_inset,0_18px_40px_-28px_rgba(15,24,39,0.5)]">
      <div className="flex h-9 items-center justify-between border-b border-[var(--dark-line)] px-4">
        <span className="font-[family-name:var(--font-mono)] text-[11px] tracking-[0.12em] text-[var(--dark-muted)] uppercase">
          {label}
        </span>
        {status ? (
          <span className="flex items-center gap-2 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-muted)]">
            <span className="live inline-block h-1.5 w-1.5 rounded-full bg-[var(--signal)]" />
            {status}
          </span>
        ) : null}
      </div>
      <pre className="term px-4 py-3.5 sm:px-5">
        <code>{children}</code>
      </pre>
    </div>
  )
}
