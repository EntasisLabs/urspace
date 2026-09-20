import { useState, type ReactNode } from 'react'
import { BrandWordmark } from './BrandMark'

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
      window.setTimeout(() => setCopied(null), 1600)
    } catch {
      setCopied(null)
    }
  }

  return (
    <section id="install" className="bg-[var(--dark)] text-[var(--dark-text)]">
      <div className="wrap py-20 sm:py-28">
        <div className="grid gap-12 lg:grid-cols-12 lg:gap-10">
          <div className="lg:col-span-5">
            <BrandWordmark onDark className="h-8" />
            <h2 className="display mt-8 text-[clamp(1.9rem,3.6vw,2.75rem)] font-semibold leading-[1.08] text-white">
              Install once.
              <br />
              Share in one command.
            </h2>
            <p className="mt-5 max-w-sm text-[15.5px] leading-relaxed text-[var(--dark-muted)]">
              Prebuilt binaries for macOS, Linux, and Windows. The installer
              verifies the download and places{' '}
              <code className="font-[family-name:var(--font-mono)] text-[0.92em] text-[var(--dark-text)]">
                urspace
              </code>{' '}
              in{' '}
              <code className="font-[family-name:var(--font-mono)] text-[0.92em] text-[var(--dark-text)]">
                ~/.local/bin
              </code>
              .
            </p>
            <p className="mt-4 max-w-sm text-[15.5px] leading-relaxed text-[var(--dark-muted)]">
              When you are done, press Ctrl+C. The site is gone. Need it to
              outlive the terminal?{' '}
              <code className="font-[family-name:var(--font-mono)] text-[0.92em] text-[var(--dark-text)]">
                urspace service
              </code>{' '}
              keeps a share running.
            </p>
            <p className="mt-6 text-[13px] leading-relaxed text-[var(--dark-faint)]">
              Early preview. Chrome and Chromium-based browsers are the main
              guest path today. Windows builds ship as a zip on{' '}
              <a
                href="https://github.com/EntasisLabs/urspace/releases"
                target="_blank"
                rel="noreferrer"
                className="text-[var(--dark-muted)] underline decoration-[var(--dark-line)] underline-offset-4 transition-colors hover:text-white hover:decoration-white"
              >
                GitHub Releases
              </a>
              .
            </p>
          </div>

          <div className="space-y-4 lg:col-span-7">
            <CommandBlock
              step="1"
              label="Install"
              command={installCmd}
              copied={copied === 'install'}
              onCopy={() => copy(installCmd, 'install')}
            >
              curl --proto '=https' --tlsv1.2 -fsSL \
              {'\n  '}
              https://github.com/<wbr />EntasisLabs/<wbr />urspace/<wbr />releases/<wbr />latest/<wbr />download/<wbr />install-urspace.sh \
              {'\n  '}| bash
            </CommandBlock>
            <CommandBlock
              step="2"
              label="Share"
              command={serveCmd}
              copied={copied === 'serve'}
              onCopy={() => copy(serveCmd, 'serve')}
            />
          </div>
        </div>
      </div>
    </section>
  )
}

function CommandBlock({
  step,
  label,
  command,
  copied,
  onCopy,
  children,
}: {
  step: string
  label: string
  command: string
  copied: boolean
  onCopy: () => void
  children?: ReactNode
}) {
  return (
    <div className="overflow-hidden rounded-[10px] border border-[var(--dark-line)] bg-[var(--dark-2)]">
      <div className="flex h-10 items-center justify-between border-b border-[var(--dark-line)] px-4">
        <span className="flex items-center gap-3 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-muted)]">
          <span className="num text-[var(--dark-faint)]">{step}</span>
          {label}
        </span>
        <button
          type="button"
          onClick={onCopy}
          aria-live="polite"
          className="rounded px-2 py-1 font-[family-name:var(--font-mono)] text-[11px] tracking-[0.04em] text-[var(--dark-muted)] transition-colors hover:bg-[var(--dark-3)] hover:text-white"
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre className="term overflow-x-auto px-4 py-4 sm:px-5">
        <code>
          <span className="p">$</span> {children ?? command}
        </code>
      </pre>
    </div>
  )
}
