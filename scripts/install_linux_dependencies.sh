#!/usr/bin/env bash
# Bern needs DBus; the desktop engine's native file picker needs Wayland.
set -euo pipefail

packages=(build-essential pkg-config cmake libdbus-1-dev libwayland-dev)
case "${1:-core}" in
  core) ;;
  gtk) packages+=(libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev) ;;
  *) printf 'Usage: %s [core|gtk]\n' "$0" >&2; exit 2 ;;
esac

apt_options=(
  -o Acquire::Retries=3
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
  -o DPkg::Lock::Timeout=120
  -o Dpkg::Use-Pty=0
)
sudo env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a \
  apt-get "${apt_options[@]}" update
sudo env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a \
  apt-get "${apt_options[@]}" -o Dpkg::Options::=--force-confdef \
  -o Dpkg::Options::=--force-confold install -y --no-install-recommends "${packages[@]}"
pkg-config --modversion dbus-1 wayland-client
if [[ "${1:-core}" == gtk ]]; then
  pkg-config --modversion gtk4 libadwaita-1 webkitgtk-6.0
fi
