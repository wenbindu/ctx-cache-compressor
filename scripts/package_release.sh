#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-${ROOT_DIR}/dist}"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "${ROOT_DIR}/Cargo.toml")"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
TARGET_TRIPLE="${TARGET:-${HOST_TARGET}}"
PKG_NAME="ctx-cache-compressor-${VERSION}-${TARGET_TRIPLE}"
STAGE_DIR="${DIST_DIR}/${PKG_NAME}"
ARCHIVE_PATH="${DIST_DIR}/${PKG_NAME}.tar.gz"
BINARY_PATH="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/ctx-cache-compressor"

mkdir -p "${DIST_DIR}"

cargo build --release --locked --target "${TARGET_TRIPLE}" --manifest-path "${ROOT_DIR}/Cargo.toml"

rm -rf "${STAGE_DIR}"
mkdir -p \
  "${STAGE_DIR}/bin" \
  "${STAGE_DIR}/config" \
  "${STAGE_DIR}/deploy/systemd"

cp "${BINARY_PATH}" "${STAGE_DIR}/bin/"
cp "${ROOT_DIR}/README.md" "${STAGE_DIR}/"
cp "${ROOT_DIR}/config.example.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/config/prod.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/config/prod-1000.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/systemd/ctx-cache-compressor.service" "${STAGE_DIR}/deploy/systemd/"
cp "${ROOT_DIR}/deploy/systemd/ctx-cache-compressor.env.example" "${STAGE_DIR}/deploy/systemd/"

tar -czf "${ARCHIVE_PATH}" -C "${DIST_DIR}" "${PKG_NAME}"

if command -v shasum >/dev/null 2>&1; then
  (
    cd "${DIST_DIR}"
    shasum -a 256 "$(basename "${ARCHIVE_PATH}")" > "$(basename "${ARCHIVE_PATH}").sha256"
  )
elif command -v sha256sum >/dev/null 2>&1; then
  (
    cd "${DIST_DIR}"
    sha256sum "$(basename "${ARCHIVE_PATH}")" > "$(basename "${ARCHIVE_PATH}").sha256"
  )
fi

echo "target=${TARGET_TRIPLE}"
echo "package=${ARCHIVE_PATH}"
