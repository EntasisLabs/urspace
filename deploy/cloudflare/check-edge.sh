#!/usr/bin/env bash
set -euo pipefail

base_domain="${URSPACE_BASE_DOMAIN:-urspace.online}"
health_host="edge-health.${base_domain}"

curl --fail --show-error --silent \
  --proto '=https' \
  --tlsv1.2 \
  "https://${health_host}/healthz" \
  | grep --fixed-strings --line-regexp 'ok'

invalid_status="$(curl --show-error --silent \
  --proto '=https' \
  --tlsv1.2 \
  --output /dev/null \
  --write-out '%{http_code}' \
  "https://not-a-valid-iroh-key.${base_domain}/.medousa/open/")"

if [[ "${invalid_status}" != "421" ]]; then
  echo "expected invalid site host to return 421, received ${invalid_status}" >&2
  exit 1
fi

if [[ $# -eq 1 ]]; then
  curl --fail --show-error --silent --head \
    --proto '=https' \
    --tlsv1.2 \
    "https://${1}.${base_domain}/.medousa/open/"
elif [[ $# -gt 1 ]]; then
  echo "usage: $0 [iroh-site-id]" >&2
  exit 2
fi

echo "Urspace edge checks passed for ${base_domain}."
