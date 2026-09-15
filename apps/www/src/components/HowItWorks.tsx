import { motion, useReducedMotion } from 'framer-motion'

const steps = [
  {
    n: '01',
    title: 'Point at loopback',
    body: 'Run urspace against an app already listening on localhost. Only 127.0.0.1, ::1, or localhost are accepted.',
  },
  {
    n: '02',
    title: 'Send an invitation',
    body: 'Urspace prints a hard-to-guess link. Treat it like a temporary password—set TTL and session limits.',
  },
  {
    n: '03',
    title: 'Guest opens it',
    body: 'They use Chrome. No Urspace account, CLI, or network access. After admission, refresh and reconnect keep working until you kick them or stop the host.',
  },
]

export function HowItWorks() {
  const reduce = useReducedMotion()

  return (
    <section id="how" className="border-t border-[var(--line)] bg-[var(--bg-elevated)]">
      <div className="mx-auto max-w-6xl px-5 py-20 sm:px-8 sm:py-28">
        <p className="font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[0.2em] text-[var(--muted)]">
          / how it works
        </p>
        <h2 className="mt-4 max-w-2xl text-3xl font-semibold tracking-tight text-[var(--ink)] sm:text-4xl">
          Invitation in. App stays home.
        </h2>
        <p className="mt-4 max-w-2xl text-base leading-relaxed text-[var(--muted)] sm:text-lg">
          You share access to one local origin—not a public server. Cloudflare
          may help the browser start the session; it does not proxy your app.
        </p>

        <ol className="mt-14 grid gap-10 md:grid-cols-3 md:gap-8">
          {steps.map((step, index) => (
            <motion.li
              key={step.n}
              initial={reduce ? false : { opacity: 0, y: 20 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: '-10%' }}
              transition={{
                duration: 0.55,
                delay: index * 0.08,
                ease: [0.22, 1, 0.36, 1],
              }}
              className="relative"
            >
              <span className="font-[family-name:var(--font-mono)] text-[12px] tracking-[0.16em] text-[var(--accent)]">
                {step.n}
              </span>
              <h3 className="mt-3 text-xl font-semibold tracking-tight text-[var(--ink)]">
                {step.title}
              </h3>
              <p className="mt-3 text-[15px] leading-relaxed text-[var(--muted)]">
                {step.body}
              </p>
            </motion.li>
          ))}
        </ol>
      </div>
    </section>
  )
}
