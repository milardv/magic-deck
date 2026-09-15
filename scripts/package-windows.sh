#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  echo "MinGW est requis pour compiler Windows. Installation via sudo..."
  sudo apt-get update
  sudo apt-get install -y mingw-w64 zip
fi

source "$HOME/.cargo/env" 2>/dev/null || true
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu

package_dir="$repo_dir/dist/magic-deck-windows-x86_64"
archive="$repo_dir/magic-deck-windows-x86_64.zip"
rm -rf "$package_dir"
mkdir -p "$package_dir"
cp "$repo_dir/target/x86_64-pc-windows-gnu/release/magic-deck.exe" "$package_dir/"

# Le binaire Rust est autonome ; ces deux DLL fournissent le runtime MinGW.
for dll in libgcc_s_seh-1.dll libwinpthread-1.dll; do
  path="$(x86_64-w64-mingw32-gcc -print-file-name="$dll")"
  if [[ -f "$path" ]]; then
    cp "$path" "$package_dir/"
  fi
done

cat > "$package_dir/README-Windows.txt" <<'EOF'
Magic Deck — Windows x86_64

1. Double-cliquez sur magic-deck.exe (ou lancez-le depuis PowerShell).
2. Ouvrez http://127.0.0.1:8092 dans votre navigateur.
3. Dans Réglages, indiquez le chemin de Player.log si nécessaire.

Le programme ne contient aucune clé API. Configurez la clé Gemini depuis Réglages
ou avec la variable GEMINI_API_KEY. Les données MTGA restent locales.
EOF

rm -f "$archive"
(cd "$repo_dir/dist" && zip -qr "$archive" "$(basename "$package_dir")")
echo "Archive créée : $archive"
