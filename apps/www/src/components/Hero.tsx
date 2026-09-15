import { motion, useReducedMotion } from 'framer-motion'
import { BrandMark } from './BrandMark'
import { ConnectionField } from './ConnectionField'

export function Hero() {
  const reduce = useReducedMotion()
  const enter = (delay = 0) =>
    reduce
      ? undefined
      : {
          initial: { opacity: 0.01, y: 16 },
          animate: { opacity: 1, y: 0 },
          transition: {
            duration: 0.65,
            delay,
            ease: [0.22, 1, 0.36, 1] as const,
          },
        }

  return (
    <section
      id="top"
      className="relative isolate min-h-[calc(100svh-3.5rem)] overflow-hidden"
    >
      <div className="pointer-events-none absolute inset-0 grid-mesh" aria-hidden />
      <div className="pointer-events-none absolute inset-x-0 top-[8%] bottom-[18%] opacity-[0.55] sm:opacity-80 lg:inset-y-0 lg:left-[38%] lg:right-0 lg:opacity-100">
        <ConnectionField />
      </div>
      <div
        className="pointer-events-none absolute inset-0 bg-gradient-to-b from-[color-mix(in_srgb,var(--bg)_55%,transparent)] via-transparent to-[var(--bg)] lg:bg-gradient-to-r lg:from-[var(--bg)] lg:via-[color-mix(in_srgb,var(--bg)_72%,transparent)] lg:to-transparent"
        aria-hidden
      />

      <div className="relative mx-auto flex min-h-[calc(100svh-3.5rem)] max-w-6xl flex-col justify-center px-5 py-16 sm:px-8 sm:py-20">
        <motion.div {...enter(0)} className="mb-6">
          <BrandMark className="h-12 w-12 sm:h-14 sm:w-14" />
        </motion.div>

        <motion.p
          {...enter(0.04)}
          className="mb-5 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.22em] text-[#4f5561]"
        >
          A private space between peers.
        </motion.p>

        <motion.h1
          {...enter(0.08)}
          className="max-w-[11ch] text-[clamp(3.6rem,13vw,7.75rem)] font-semibold leading-[0.88] tracking-[-0.05em] text-[#0f1827]"
        >
          urspace
        </motion.h1>

        <motion.p
          {...enter(0.14)}
          className="mt-6 max-w-md text-lg leading-relaxed text-[#4f5561] sm:max-w-lg sm:text-xl"
        >
          A private encrypted path to the app on your machine. Guests open one
          invitation in a browser—no account, VPN, or open ports.
        </motion.p>

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
            className="inline-flex items-center border border-[#0f1827]/30 bg-[color-mix(in_srgb,#f4f6f7_70%,transparent)] px-5 py-3.5 font-[family-name:var(--font-mono)] text-[13px] font-medium uppercase tracking-[0.14em] text-[#0f1827] backdrop-blur-sm transition-colors duration-200 hover:border-[#0f1827] hover:bg-[#f4f6f7]"
          >
            GitHub
          </a>
        </div>
      </div>
    </section>
  )
}
