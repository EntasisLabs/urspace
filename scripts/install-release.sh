#!/usr/bin/env bash
set -euo pipefail

repo="${URSPACE_GITHUB_REPOSITORY:-EntasisLabs/urspace}"
version="${1:-latest}"
install_dir="${URSPACE_INSTALL_DIR:-${HOME}/.local/bin}"

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI is required for private-preview downloads: https://cli.github.com" >&2
  exit 1
fi

case "$(uname -s):$(uname -m)" in
  Darwin:x86_64) target="x86_64-apple-darwin" ;;
  Darwin:arm64) target="aarch64-apple-darwin" ;;
  Linux:x86_64) target="x86_64-unknown-linux-gnu" ;;
  Linux:aarch64 | Linux:arm64) target="aarch64-unknown-linux-gnu" ;;
  *)
    echo "No Urspace preview binary is published for $(uname -s) $(uname -m)." >&2
    exit 1
    ;;
esac

download_dir="$(mktemp -d)"
cleanup() {
  rm -rf "${download_dir}"
}
trap cleanup EXIT

if [[ "${version}" == "latest" ]]; then
  gh release download \
    --repo "${repo}" \
    --pattern "urspace-*-${target}.tar.gz" \
    --pattern SHA256SUMS \
    --dir "${download_dir}"
else
  gh release download "${version}" \
    --repo "${repo}" \
    --pattern "urspace-${version}-${target}.tar.gz" \
    --pattern SHA256SUMS \
    --dir "${download_dir}"
fi

archive="$(find "${download_dir}" -maxdepth 1 -name "urspace-*-${target}.tar.gz" -print -quit)"
if [[ -z "${archive}" ]]; then
  echo "The selected release does not contain a binary for ${target}." >&2
  exit 1
fi

archive_name="$(basename "${archive}")"
expected="$(awk -v name="${archive_name}" '$2 == name { print $1 }' "${download_dir}/SHA256SUMS")"
if [[ -z "${expected}" ]]; then
  echo "SHA256SUMS does not contain ${archive_name}." >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${archive}" | awk '{ print $1 }')"
else
  actual="$(shasum -a 256 "${archive}" | awk '{ print $1 }')"
fi
if [[ "${actual}" != "${expected}" ]]; then
  echo "Checksum verification failed for ${archive_name}." >&2
  exit 1
fi

extract_dir="${download_dir}/extract"
mkdir -p "${extract_dir}" "${install_dir}"
tar -C "${extract_dir}" -xzf "${archive}"
install -m 0755 "${extract_dir}/urspace" "${install_dir}/urspace"

echo "Installed Urspace to ${install_dir}/urspace"
if [[ ":${PATH}:" != *":${install_dir}:"* ]]; then
  echo "Add ${install_dir} to PATH, then run: urspace --help"
else
  echo "Run: urspace --help"
fi
