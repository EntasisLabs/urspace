import { useState } from 'react'

const installCmd = `curl --proto '=https' --tlsv1.2 -fsSL \\
  https://github.com/EntasisLabs/urspace/releases/latest/download/install-urspace.sh \\
  | bash`

const serveCmd = 'urspace serve localhost:8787 --short'

export function Install() {
  const [copied, setCopied] = useState<'install' | 'serve' | null>(null)

  async function copy(text: string, key: 'install' | 'serve') {
    try {
      await navigator.clipboard.writeText(text.replace(/\\\n\s*/g, ''))
      setCopied(key)
      window.setTimeout(() => setCopied(null), 1800)
    } catch {
      setCopied(null)
    }
  }

  return (
    <section id="install" className="border-t border-[var(--line)] bg-[var(--bg-elevated)]">
      <div className="mx-auto max-w-6xl px-5 py-20 sm:px-8 sm:py-28">
        <p className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.2em] text-[var(--muted)]">
          / install
        </p>
        <h2 className="mt-4 max-w-xl text-3xl font-semibold tracking-tight text-[var(--ink)] sm:text-4xl">
          Install once. Share in one command.
        </h2>
        <p className="mt-4 max-w-2xl text-base leading-relaxed text-[var(--muted)] sm:text-lg">
          Prebuilt releases for macOS, Linux, and Windows. The installer lands in{' '}
          <code className="font-[family-name:var(--font-mono)] text-[0.92em] text-[var(--ink)]">
            ~/.local/bin
          </code>{' '}
          and verifies the download.
        </p>

        <div className="mt-12 space-y-6">
          <CommandBlock
            label="macOS / Linux"
            command={installCmd}
            copied={copied === 'install'}
            onCopy={() => copy(installCmd, 'install')}
          />
          <CommandBlock
            label="then share"
            command={serveCmd}
            copied={copied === 'serve'}
            onCopy={() => copy(serveCmd, 'serve')}
          />
        </div>

        <p className="mt-8 text-sm leading-relaxed text-[var(--muted)]">
          Windows: download the zip from{' '}
          <a
            href="https://github.com/EntasisLabs/urspace/releases"
            target="_blank"
            rel="noreferrer"
            className="underline decoration-[var(--line-strong)] underline-offset-4 transition-colors hover:text-[var(--ink)] hover:decoration-[var(--ink)]"
          >
            GitHub Releases
          </a>
          . Early preview—Chrome and Chromium-based browsers are the main path today.
        </p>
      </div>
    </section>
  )
}

function CommandBlock({
  label,
  command,
  copied,
  onCopy,
}: {
  label: string
  command: string
  copied: boolean
  onCopy: () => void
}) {
  return (
    <div className="overflow-hidden border border-[var(--line)] bg-[var(--dark)]">
      <div className="flex items-center justify-between border-b border-[var(--dark-line)] px-4 py-2.5">
        <span className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.16em] text-[var(--dark-muted)]">
          {label}
        </span>
        <button
          type="button"
          onClick={onCopy}
          className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.14em] text-[var(--signal)] transition-opacity hover:opacity-80"
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre className="overflow-x-auto px-4 py-4 font-[family-name:var(--font-mono)] text-[13px] leading-relaxed text-[#d7dde2]">
        <code>{command}</code>
      </pre>
    </div>
  )
}
