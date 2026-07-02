#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-${ROOT_DIR}/dist}"
VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "${ROOT_DIR}/Cargo.toml")"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
TARGET_TRIPLE="${TARGET:-${HOST_TARGET}}"
BINARIES="${BINARIES:-ctx-cache-compressor-api ctx-cache-compressor-demo ctx-cache-compressor}"
PKG_NAME="ctx-cache-compressor-${VERSION}-${TARGET_TRIPLE}"
STAGE_DIR="${DIST_DIR}/${PKG_NAME}"
ARCHIVE_PATH="${DIST_DIR}/${PKG_NAME}.tar.gz"
TARGET_RELEASE_DIR="${ROOT_DIR}/target/${TARGET_TRIPLE}/release"

if [[ "${TARGET_TRIPLE}" == *"windows"* ]]; then
  EXE_SUFFIX=".exe"
else
  EXE_SUFFIX=""
fi

mkdir -p "${DIST_DIR}"

build_args=(cargo build --release --locked --target "${TARGET_TRIPLE}" --manifest-path "${ROOT_DIR}/Cargo.toml")
for binary in ${BINARIES}; do
  build_args+=(--bin "${binary}")
done
"${build_args[@]}"

rm -rf "${STAGE_DIR}"
mkdir -p \
  "${STAGE_DIR}/bin" \
  "${STAGE_DIR}/config" \
  "${STAGE_DIR}/deploy/systemd" \
  "${STAGE_DIR}/docs" \
  "${STAGE_DIR}/scripts"

for binary in ${BINARIES}; do
  binary_path="${TARGET_RELEASE_DIR}/${binary}${EXE_SUFFIX}"
  if [[ ! -x "${binary_path}" ]]; then
    echo "release binary not found after build: ${binary_path}" >&2
    exit 1
  fi
  cp "${binary_path}" "${STAGE_DIR}/bin/"
done

cp "${ROOT_DIR}/README.md" "${STAGE_DIR}/"
cp "${ROOT_DIR}/README.zh-CN.md" "${STAGE_DIR}/"
if [[ -f "${ROOT_DIR}/LICENSE" ]]; then
  cp "${ROOT_DIR}/LICENSE" "${STAGE_DIR}/"
fi
cp "${ROOT_DIR}/config.example.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/config/prod.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/config/prod-1000.toml" "${STAGE_DIR}/config/"
cp "${ROOT_DIR}/deploy/systemd/ctx-cache-compressor.service" "${STAGE_DIR}/deploy/systemd/"
cp "${ROOT_DIR}/deploy/systemd/ctx-cache-compressor.env.example" "${STAGE_DIR}/deploy/systemd/"
cp "${ROOT_DIR}/scripts/run_release.sh" "${STAGE_DIR}/scripts/"
cp "${ROOT_DIR}/scripts/smoke.sh" "${STAGE_DIR}/scripts/"
cp "${ROOT_DIR}/docs/release.md" "${STAGE_DIR}/docs/" 2>/dev/null || true

cat > "${STAGE_DIR}/MANIFEST.txt" <<EOF
ctx-cache-compressor ${VERSION}
target: ${TARGET_TRIPLE}
binaries: ${BINARIES}

Default production entrypoint:
  bin/ctx-cache-compressor-api${EXE_SUFFIX}

Config:
  config/prod.toml
  config/prod-1000.toml
  OPENAI_API_KEY or [llm].api_key

Run:
  CTX_CACHE_COMPRESSOR_CONFIG_FILE=config/prod.toml bin/ctx-cache-compressor-api${EXE_SUFFIX}
EOF

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
