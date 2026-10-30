#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Build for the current host. GNU/Linux archives require compatible glibc.
package_host="$(rustc -vV | sed -n 's/^host: //p')"
case "$package_host" in
  *-unknown-linux-gnu) ;;
  *) echo 'Packaging supports GNU/Linux hosts only.' >&2; exit 1 ;;
esac
if [[ -n "${CARGO_BUILD_TARGET:-}" && "$CARGO_BUILD_TARGET" != "$package_host" ]]; then
  echo 'Cross-compiled releases are not supported by this script.' >&2
  exit 1
fi
package_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
cargo build --release --locked
package_binary="${CARGO_TARGET_DIR:-target}"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then package_binary="$package_binary/$package_host"; fi
package_binary="$package_binary/release/hyprkey"
package_name="hyprkey-${package_version}-${package_host}"
package_stage="$(mktemp -d)"
trap 'rm -rf -- "$package_stage"' EXIT
mkdir -p "dist" "$package_stage/$package_name/assets/fonts"
install -m755 "$package_binary" "$package_stage/$package_name/hyprkey"
cp LICENSE README.md CHANGELOG.md "$package_stage/$package_name/"
cp assets/fonts/OFL.txt assets/fonts/README.md "$package_stage/$package_name/assets/fonts/"
cp assets/preview.png "$package_stage/$package_name/assets/"
tar -czf "dist/$package_name.tar.gz" -C "$package_stage" "$package_name"
(cd dist && sha256sum "$package_name.tar.gz" > "$package_name.tar.gz.sha256")
printf 'Created dist/%s.tar.gz and checksum\n' "$package_name"
