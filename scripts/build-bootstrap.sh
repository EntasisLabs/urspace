#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

wasm_cc="${CC_wasm32_unknown_unknown:-}"
if [[ -z "${wasm_cc}" && -x /opt/homebrew/opt/llvm/bin/clang ]]; then
  wasm_cc=/opt/homebrew/opt/llvm/bin/clang
fi
if [[ -z "${wasm_cc}" ]]; then
  wasm_cc=clang
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen CLI 0.2.122 is required" >&2
  echo "install it with: cargo install wasm-bindgen-cli --version 0.2.122 --locked" >&2
  exit 1
fi

env CC_wasm32_unknown_unknown="${wasm_cc}" \
  cargo build -p urspace-browser --target wasm32-unknown-unknown --release

wasm-bindgen \
  target/wasm32-unknown-unknown/release/urspace_browser.wasm \
  --out-dir apps/bootstrap/public/.urspace/wasm \
  --target web \
  --weak-refs
