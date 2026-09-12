#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [boxclub-directory]" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
boxclub_dir="${1:-/Users/theelevators/boxclub/BoxClub}"
boxclub_port="${BOXCLUB_PORT:-8787}"
bootstrap_port="${MEDOUSA_SITES_BOOTSTRAP_PORT:-8080}"

if [[ ! -f "${boxclub_dir}/package.json" ]]; then
  echo "BoxClub package.json not found under ${boxclub_dir}" >&2
  exit 2
fi

cleanup() {
  jobs -pr | xargs kill 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "Building BoxClub and the browser bootstrap..."
(cd "${boxclub_dir}" && npm run build)
(cd "${repo_root}/apps/bootstrap" && npm run build)

echo "Starting BoxClub on loopback port ${boxclub_port}..."
(cd "${boxclub_dir}" && env NODE_ENV=production PORT="${boxclub_port}" npm start) &
boxclub_pid=$!

echo "Starting the local bootstrap on port ${bootstrap_port}..."
python3 -m http.server "${bootstrap_port}" \
  --bind 127.0.0.1 \
  --directory "${repo_root}/apps/bootstrap/public" &
bootstrap_pid=$!

for _ in {1..50}; do
  if curl --fail --silent "http://127.0.0.1:${boxclub_port}/api/health" >/dev/null; then
    break
  fi
  if ! kill -0 "${boxclub_pid}" 2>/dev/null || ! kill -0 "${bootstrap_pid}" 2>/dev/null; then
    echo "A local server exited before startup completed." >&2
    exit 1
  fi
  sleep 0.1
done
curl --fail --silent "http://127.0.0.1:${boxclub_port}/api/health" >/dev/null

echo "Minting a 1-hour BoxClub invite. Press Ctrl+C to stop all three processes."
cd "${repo_root}"
cargo run -p medousa-site-host --bin urspace -- serve "localhost:${boxclub_port}" \
  --bootstrap-origin "http://localhost:${bootstrap_port}" \
  --name boxclub \
  --ttl 1h \
  --max-sessions 4
