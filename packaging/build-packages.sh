#!/bin/bash
# MailNix paketlerini (.deb, .rpm, .AppImage) en eski desteklenen tabanda derler.
#
# Paketler Ubuntu 22.04 (glibc 2.35) içinde derlenir; böylece
# Pardus 23/25, Debian 12/13, Ubuntu 22.04/24.04 ve sonrasında çalışır.
#
# Kullanım:  packaging/build-packages.sh            (Docker gerekir)
# Çıktı:     dist/
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${MAILNIX_BUILD_IMAGE:-ubuntu:22.04}"

if [ "${1:-}" != "--inside" ]; then
  mkdir -p "$ROOT/dist" "$ROOT/.build-cache"
  extra=()
  [ -f /root/.ccr/ca-bundle.crt ] && extra+=(-v /root/.ccr/ca-bundle.crt:/etc/ssl/certs/proxy-ca.crt:ro -e PROXY_CA=/etc/ssl/certs/proxy-ca.crt)
  [ -d "${CARGO_HOME:-$HOME/.cargo}/registry" ] && extra+=(-v "${CARGO_HOME:-$HOME/.cargo}/registry:/root/.cargo/registry")
  # Ana makinedeki araç zincirleri (taşınabilir ikili dosyalar) varsa kullanılır
  [ -d "${CARGO_HOME:-$HOME/.cargo}/bin" ] && extra+=(-v "${CARGO_HOME:-$HOME/.cargo}/bin:/root/.cargo/bin:ro")
  [ -d "${RUSTUP_HOME:-$HOME/.rustup}" ] && extra+=(-v "${RUSTUP_HOME:-$HOME/.rustup}:/opt/rustup:ro" -e HOST_RUSTUP=1)
  NODE_BIN="$(command -v node || true)"
  [ -n "$NODE_BIN" ] && extra+=(-v "$(dirname "$(dirname "$(readlink -f "$NODE_BIN")")"):/opt/hostnode:ro")
  exec docker run --rm --network host \
    -e HTTPS_PROXY="${HTTPS_PROXY:-}" -e https_proxy="${HTTPS_PROXY:-}" -e NO_PROXY="${NO_PROXY:-}" -e no_proxy="${NO_PROXY:-}" \
    -e MAILNIX_MS_CLIENT_ID="${MAILNIX_MS_CLIENT_ID:-}" -e MAILNIX_GOOGLE_CLIENT_ID="${MAILNIX_GOOGLE_CLIENT_ID:-}" \
    "${extra[@]}" \
    -v "$ROOT:/src" -v "$ROOT/.build-cache:/cache" \
    "$IMAGE" bash /src/packaging/build-packages.sh --inside
fi

# ------------------------------------------------------------ kapsayıcı içinde
export DEBIAN_FRONTEND=noninteractive
if [ -n "${PROXY_CA:-}" ]; then
  echo "Acquire::https::CAInfo \"$PROXY_CA\";" > /etc/apt/apt.conf.d/99proxy-ca
  export SSL_CERT_FILE="$PROXY_CA" CURL_CA_BUNDLE="$PROXY_CA" NODE_EXTRA_CA_CERTS="$PROXY_CA" CARGO_HTTP_CAINFO="$PROXY_CA"
fi
if [ -n "${HTTPS_PROXY:-}" ]; then
  # Yalnızca HTTPS vekil sunucusu varsa depoları https üzerinden kullan
  sed -i 's#http://archive.ubuntu.com#https://archive.ubuntu.com#g; s#http://security.ubuntu.com#https://security.ubuntu.com#g' /etc/apt/sources.list
  echo "Acquire::https::Proxy \"$HTTPS_PROXY\";" > /etc/apt/apt.conf.d/99proxy
fi
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  build-essential curl ca-certificates file pkg-config xz-utils patchelf rpm desktop-file-utils \
  libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  librsvg2-dev libayatana-appindicator3-dev libssl-dev libdbus-1-dev >/dev/null

export CARGO_HOME=/root/.cargo RUSTUP_HOME=/cache/rustup
[ -n "${HOST_RUSTUP:-}" ] && export RUSTUP_HOME=/opt/rustup
export PATH=/root/.cargo/bin:/opt/hostnode/bin:/cache/node/bin:$PATH
if ! command -v cargo >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path >/dev/null
fi
if ! command -v node >/dev/null && [ ! -x /cache/node/bin/node ]; then
  mkdir -p /cache/node
  curl -fsSL https://nodejs.org/dist/v22.12.0/node-v22.12.0-linux-x64.tar.xz | tar -xJ -C /cache/node --strip-components=1
fi

cd /src
export CARGO_TARGET_DIR=/cache/target
# linuxdeploy bir AppImage; kapsayıcıda FUSE olmadığından açılarak çalıştırılır
export APPIMAGE_EXTRACT_AND_RUN=1
# appimagetool çalışma zamanını kendi indirir; vekil sunucu arkasında önceden indirip veriyoruz
if [ ! -s /cache/runtime-x86_64 ]; then
  curl -fsSL -o /cache/runtime-x86_64 https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64 || rm -f /cache/runtime-x86_64
fi
[ -s /cache/runtime-x86_64 ] && export LDAI_RUNTIME_FILE=/cache/runtime-x86_64
npm ci --no-audit --no-fund
npx tauri build --bundles deb,rpm,appimage --verbose > /cache/tauri-build.log 2>&1 || { grep -iv "^\s*\(compiling\|running\)" /cache/tauri-build.log | tail -40; exit 1; }
B=/cache/target/release/bundle
VER=$(node -p "require('/src/src-tauri/tauri.conf.json').version")
DEB=$B/deb/MailNix_${VER}_amd64.deb

# .deb'ye bildirim alanı kitaplıkları gömülür (sistemden ek paket istenmez):
# /usr/lib/mailnix altına konur, uygulama onları önce orada arar (build.rs: rpath)
EMBED_LIBS="libayatana-appindicator3.so.1 libayatana-indicator3.so.7 libayatana-ido3-0.4.so.0 libdbusmenu-glib.so.4 libdbusmenu-gtk3.so.4"
R=$(mktemp -d)
dpkg-deb -R "$DEB" "$R"
mkdir -p "$R/usr/lib/mailnix"
for l in $EMBED_LIBS; do
  src=$(ldconfig -p | awk -v l="$l" '$1==l && !f{print $NF; f=1}')
  [ -n "$src" ] || { echo "gömülecek kitaplık bulunamadı: $l"; exit 1; }
  install -m 0644 "$(readlink -f "$src")" "$R/usr/lib/mailnix/$l"
done
mkdir -p "$R/usr/share/doc/mail-nix"
dpkg-query -W -f='${Package} ${Version}\n' libayatana-appindicator3-1 libayatana-indicator3-7 libayatana-ido3-0.4-0 libdbusmenu-glib4 libdbusmenu-gtk3-4 > "$R/usr/share/doc/mail-nix/embedded-libraries.txt"
# Gömülen kitaplığın paketi bağımlılıktan çıkar
sed -i -E 's/(^Depends: .*)libayatana-appindicator3-1(, )?/\1/; s/, *$//' "$R/DEBIAN/control"
sed -i -E "s/^Installed-Size: .*/Installed-Size: $(du -sk --exclude=DEBIAN "$R" | cut -f1)/" "$R/DEBIAN/control"
(cd "$R" && find . -type f ! -path './DEBIAN/*' -printf '%P\0' | xargs -0 md5sum > DEBIAN/md5sums)
dpkg-deb --root-owner-group -Zxz -b "$R" "$DEB" >/dev/null
rm -rf "$R"

rm -rf /src/dist/*
cp "$DEB" $B/rpm/MailNix-${VER}-1.x86_64.rpm $B/appimage/MailNix_${VER}_amd64.AppImage /src/dist/ 2>/dev/null || true
dpkg-deb -f /src/dist/MailNix_${VER}_amd64.deb Depends
ls -la /src/dist
