import { motion } from 'framer-motion'
import { ConnectionField } from './ConnectionField'

export function Hero() {
  return (
    <section
      id="top"
      className="relative isolate min-h-[calc(100svh-3.5rem)] overflow-hidden"
    >
      <div className="pointer-events-none absolute inset-0 grid-mesh" aria-hidden />
      <ConnectionField />

      <div className="relative mx-auto flex min-h-[calc(100svh-3.5rem)] max-w-6xl flex-col justify-end px-5 pb-16 pt-20 sm:px-8 sm:pb-20 lg:justify-center lg:pb-24 lg:pt-10">
        <motion.p
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.55, ease: [0.22, 1, 0.36, 1] }}
          className="mb-5 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.22em] text-[var(--accent)]"
        >
          Private local sharing
        </motion.p>

        <motion.h1
          initial={{ opacity: 0, y: 18 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.06, ease: [0.22, 1, 0.36, 1] }}
          className="max-w-[11ch] text-[clamp(3.4rem,12vw,7.5rem)] font-semibold leading-[0.9] tracking-[-0.045em] text-[var(--ink)]"
        >
          urspace
        </motion.h1>

        <motion.p
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.65, delay: 0.16, ease: [0.22, 1, 0.36, 1] }}
          className="mt-6 max-w-xl text-lg leading-relaxed text-[var(--muted)] sm:text-xl"
        >
          A private encrypted path to the app on your machine. Guests open one
          invitation in a browser—no account, VPN, or open ports.
        </motion.p>

        <motion.div
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.28, ease: [0.22, 1, 0.36, 1] }}
          className="mt-10 flex flex-wrap items-center gap-3"
        >
          <a
            href="#install"
            className="inline-flex items-center bg-[var(--ink)] px-5 py-3 font-[family-name:var(--font-mono)] text-[12px] uppercase tracking-[0.16em] text-[var(--bg-elevated)] transition-[transform,background-color] duration-200 hover:bg-[var(--accent-ink)] active:scale-[0.98]"
          >
            Get the CLI
          </a>
          <a
            href="https://github.com/EntasisLabs/urspace"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center border border-[var(--ink)]/25 bg-transparent px-5 py-3 font-[family-name:var(--font-mono)] text-[12px] uppercase tracking-[0.16em] text-[var(--ink)] transition-colors duration-200 hover:border-[var(--ink)] hover:bg-[var(--bg-elevated)]"
          >
            GitHub
          </a>
        </motion.div>
      </div>
    </section>
  )
}
