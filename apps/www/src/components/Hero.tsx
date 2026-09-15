import { BrandMark } from './BrandMark'

export function Hero() {
  return (
    <section
      id="top"
      className="relative min-h-[calc(100svh-3.5rem)]"
    >
      <div className="mx-auto flex min-h-[calc(100svh-3.5rem)] max-w-6xl flex-col justify-center px-5 py-16 sm:px-8 sm:py-20">
        <div className="mb-6">
          <BrandMark className="h-12 w-12 sm:h-14 sm:w-14" />
        </div>

        <p className="mb-5 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.22em] text-[#4f5561]">
          A private space between peers.
        </p>

        <h1 className="max-w-[11ch] text-[clamp(3.6rem,13vw,7.75rem)] font-semibold leading-[0.88] tracking-[-0.05em] text-[#0f1827]">
          urspace
        </h1>

        <p className="mt-6 max-w-md text-lg leading-relaxed text-[#4f5561] sm:max-w-lg sm:text-xl">
          A private encrypted path to the app on your machine. Guests open one
          invitation in a browser—no account, VPN, or open ports.
        </p>

        <div className="mt-10 flex flex-wrap items-center gap-3">
          <a
            href="#install"
            className="inline-flex items-center bg-[#0f1827] px-5 py-3.5 font-[family-name:var(--font-mono)] text-[13px] font-medium uppercase tracking-[0.14em] text-white transition-[transform,background-color] duration-200 hover:bg-[#1a2433] active:scale-[0.98]"
          >
            Get the CLI
          </a>
          <a
            href="https://github.com/EntasisLabs/urspace"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center border border-[#0f1827]/30 bg-transparent px-5 py-3.5 font-[family-name:var(--font-mono)] text-[13px] font-medium uppercase tracking-[0.14em] text-[#0f1827] transition-colors duration-200 hover:border-[#0f1827] hover:bg-[#f4f6f7]"
          >
            GitHub
          </a>
        </div>
      </div>
    </section>
  )
}
