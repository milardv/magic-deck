#!/usr/bin/env bash
set -euo pipefail

# Pinned upstream desktop distribution. Forge is installed separately from the Rust binary.
forge_version=2.0.14
forge_sha=e71749376945177d603d52a21a089bc1574330cf34599544b6bae1a04c83da41
forge_parent="${XDG_DATA_HOME:-$HOME/.local/share}/magic-deck/forge"
forge_destination="$forge_parent/$forge_version"
if [[ -d "$forge_destination" ]]; then
  if [[ -f "$forge_destination/forge-gui-desktop-$forge_version-jar-with-dependencies.jar" && -d "$forge_destination/res" ]]; then
    printf 'Forge déjà installé : %s\n' "$forge_destination"
    exit 0
  fi
  printf 'Dossier existant incomplet : %s. Choisissez un autre XDG_DATA_HOME.\n' "$forge_destination" >&2
  exit 1
fi
for forge_tool in java javac tar sha256sum; do
  command -v "$forge_tool" >/dev/null || { printf 'Outil manquant : %s (JDK 17+ requis).\n' "$forge_tool" >&2; exit 1; }
done
mkdir -p "$forge_parent"
forge_staging="$(mktemp -d "$forge_parent/.install-XXXXXX")"
forge_archive="${FORGE_ARCHIVE:-$forge_staging/forge.tar.bz2}"
if [[ -z "${FORGE_ARCHIVE:-}" ]]; then
  curl --fail --location --retry 2 --output "$forge_archive" "https://github.com/Card-Forge/forge/releases/download/forge-$forge_version/forge-installer-$forge_version.tar.bz2"
fi
printf '%s  %s\n' "$forge_sha" "$forge_archive" | sha256sum --check --status
mkdir "$forge_staging/engine"
tar -xjf "$forge_archive" -C "$forge_staging/engine"
test -f "$forge_staging/engine/forge-gui-desktop-$forge_version-jar-with-dependencies.jar"
test -d "$forge_staging/engine/res"
mv -T "$forge_staging/engine" "$forge_destination"
printf 'Forge installé. Dans Simulation → Configurer le moteur, indiquez :\n%s\n' "$forge_destination"
printf 'Archive et staging conservés dans %s pour diagnostic.\n' "$forge_staging"
