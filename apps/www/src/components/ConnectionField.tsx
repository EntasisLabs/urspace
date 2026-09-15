export function ConnectionField() {
  return (
    <div
      className="pointer-events-none absolute inset-0 flex items-start justify-center pt-6 sm:items-center sm:pt-0"
      aria-hidden
    >
      <svg
        className="h-[min(72vh,640px)] w-[min(110%,920px)] opacity-90"
        viewBox="0 0 640 400"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <defs>
          <linearGradient id="pathFade" x1="80" y1="220" x2="560" y2="180" gradientUnits="userSpaceOnUse">
            <stop stopColor="#1F6F5F" stopOpacity="0.15" />
            <stop offset="0.5" stopColor="#1F6F5F" stopOpacity="0.55" />
            <stop offset="1" stopColor="#1F6F5F" stopOpacity="0.15" />
          </linearGradient>
          <filter id="soft" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur stdDeviation="1.2" />
          </filter>
        </defs>

        {/* ambient nodes */}
        {[
          [120, 70],
          [210, 40],
          [300, 55],
          [390, 35],
          [480, 65],
          [160, 330],
          [270, 350],
          [400, 340],
          [510, 310],
        ].map(([x, y], i) => (
          <circle
            key={`${x}-${y}`}
            cx={x}
            cy={y}
            r={i % 3 === 0 ? 2.2 : 1.4}
            fill="#0C0E10"
            opacity={0.12 + (i % 4) * 0.03}
          />
        ))}

        {/* faint lattice */}
        <path
          d="M140 120 L220 90 L300 130 L380 70 L460 110 M180 280 L260 310 L340 270 L420 300 L500 260"
          stroke="#0C0E10"
          strokeOpacity="0.06"
          strokeWidth="1"
        />

        {/* main encrypted path */}
        <path
          d="M 80 220 C 220 80, 420 360, 560 180"
          stroke="url(#pathFade)"
          strokeWidth="2"
          strokeDasharray="5 7"
        />
        <path
          d="M 80 220 C 220 80, 420 360, 560 180"
          stroke="#1F6F5F"
          strokeOpacity="0.25"
          strokeWidth="1"
          filter="url(#soft)"
        />

        {/* traveling packets */}
        <g className="packet">
          <circle r="4" fill="#2A8F78" />
        </g>
        <g className="packet">
          <circle r="3" fill="#143F37" />
        </g>
        <g className="packet">
          <circle r="3.5" fill="#2A8F78" opacity="0.7" />
        </g>

        {/* host node */}
        <g transform="translate(48, 178)">
          <rect width="64" height="84" rx="4" fill="#F4F6F7" stroke="#0C0E10" strokeWidth="1.5" />
          <rect x="10" y="14" width="44" height="28" rx="2" fill="#E6E9EC" stroke="#0C0E10" strokeOpacity="0.35" />
          <rect x="10" y="52" width="28" height="6" rx="1" fill="#0C0E10" opacity="0.55" />
          <rect x="10" y="64" width="18" height="4" rx="1" fill="#0C0E10" opacity="0.25" />
          <text
            x="32"
            y="102"
            textAnchor="middle"
            fill="#5A6570"
            style={{ fontFamily: 'IBM Plex Mono, monospace', fontSize: 10, letterSpacing: '0.12em' }}
          >
            HOST
          </text>
        </g>

        {/* guest browser */}
        <g transform="translate(528, 138)">
          <rect width="72" height="84" rx="4" fill="#111418" stroke="#0C0E10" strokeWidth="1.5" />
          <rect x="8" y="10" width="56" height="40" rx="2" fill="#1A1F24" />
          <circle cx="16" cy="18" r="2" fill="#2A8F78" />
          <rect x="24" y="16" width="28" height="3" rx="1" fill="#8B959E" opacity="0.5" />
          <rect x="12" y="58" width="36" height="5" rx="1" fill="#E8ECEF" opacity="0.85" />
          <rect x="12" y="68" width="24" height="4" rx="1" fill="#8B959E" opacity="0.45" />
          <text
            x="36"
            y="102"
            textAnchor="middle"
            fill="#5A6570"
            style={{ fontFamily: 'IBM Plex Mono, monospace', fontSize: 10, letterSpacing: '0.12em' }}
          >
            GUEST
          </text>
        </g>

        <text
          x="320"
          y="248"
          textAnchor="middle"
          fill="#1F6F5F"
          style={{ fontFamily: 'IBM Plex Mono, monospace', fontSize: 11, letterSpacing: '0.18em' }}
        >
          E2E ENCRYPTED · IROH
        </text>
      </svg>
    </div>
  )
}
