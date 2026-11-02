#!/usr/bin/env bash
# Release notes of one version: its section of CHANGELOG.md plus the download and install notes.
# Usage: tools/release_notes.sh 0.2.0-beta   (used by .github/workflows/release.yml)
set -euo pipefail
version="${1:?version, e.g. 0.2.0-beta}"
root="$(cd "$(dirname "$0")/.." && pwd)"
section="$(awk -v v="$version" '
  /^## / { if (found) exit; if (index($0, "## " v " ") == 1) { found = 1; next } }
  found { print }
' "$root/CHANGELOG.md" | sed -e '/./,$!d')"
if [ -z "$section" ]; then
  echo "CHANGELOG.md has no section for $version" >&2
  exit 1
fi
cat <<EOF
$section

## Downloads

- **macOS (Apple silicon):** \`.dmg\`, drag the app into Applications.
- **Windows 10/11:** \`x64-setup.exe\` or \`arm64-setup.exe\`.

The builds are not signed. macOS: open the app once via right-click → Open (or System Settings → Privacy & Security → Open Anyway). Windows: SmartScreen → More info → Run anyway.

Updating keeps all data (recording, installations, parameter states): it lives in the app's data folder, not in the app. See the [README](https://github.com/privatecoder/openbms-monitor#readme) for what this pre-release does and what is untested, and [CHANGELOG.md](https://github.com/privatecoder/openbms-monitor/blob/main/CHANGELOG.md) for all versions.
EOF
