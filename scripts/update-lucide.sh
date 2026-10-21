#!/usr/bin/env bash
#
# Vendor the Lucide icon set (https://lucide.dev, ISC) into assets/lucide/.
#
# Fetches the immutable `lucide-static` archive for <version> from the npm
# registry, checks it against the registry's published sha512, and keeps the
# canonical icons (those named in Lucide's tags.json; deprecated aliases are
# left out) with their tags and licence. Writes:
#
#   assets/lucide/VERSION      the version
#   assets/lucide/SOURCE       the archive's URL, sha256 and npm integrity
#   assets/lucide/LICENSE      Lucide's licence
#   assets/lucide/tags.json    Lucide's tags, name -> [tag]
#   assets/lucide/icons/*.svg  one SVG per icon
#   assets/lucide/SHA256SUMS   a sha256 per vendored file (`sha256sum -c SHA256SUMS`)
#
# Re-running with the same version reproduces every file byte for byte.
#
# Usage:
#   scripts/update-lucide.sh <version>     e.g. scripts/update-lucide.sh 1.61.0
#
# Needs: curl, jq, tar, sha256sum, sha512sum, xxd, base64.

set -euo pipefail

if [[ $# -ne 1 || ! $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/update-lucide.sh <version>   (e.g. 1.61.0)" >&2
  exit 2
fi
version=$1

for tool in curl jq tar sha256sum sha512sum xxd base64; do
  command -v "$tool" >/dev/null || { echo "update-lucide: needs $tool" >&2; exit 1; }
done

cd "$(dirname "$0")/.."
dest=assets/lucide
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "update-lucide: reading lucide-static $version from the npm registry"
curl -fsSL "https://registry.npmjs.org/lucide-static/$version" -o "$work/meta.json"
url=$(jq -r '.dist.tarball' "$work/meta.json")
integrity=$(jq -r '.dist.integrity' "$work/meta.json")
[[ $url == https://registry.npmjs.org/* && $integrity == sha512-* ]] || {
  echo "update-lucide: the registry named no archive for $version" >&2
  exit 1
}

echo "update-lucide: fetching $url"
curl -fsSL "$url" -o "$work/archive.tgz"
actual="sha512-$(sha512sum "$work/archive.tgz" | cut -d' ' -f1 | xxd -r -p | base64 -w0)"
if [[ $actual != "$integrity" ]]; then
  echo "update-lucide: the archive does not match the registry's integrity" >&2
  echo "  expected $integrity" >&2
  echo "  got      $actual" >&2
  exit 1
fi
archive_sha256=$(sha256sum "$work/archive.tgz" | cut -d' ' -f1)

tar -xzf "$work/archive.tgz" -C "$work"
package=$work/package
[[ $(jq -r '.version' "$package/package.json") == "$version" ]] || {
  echo "update-lucide: the archive is not version $version" >&2
  exit 1
}

stage=$work/lucide
mkdir -p "$stage/icons"
cp "$package/LICENSE" "$package/tags.json" "$stage/"
count=0
while IFS= read -r name; do
  [[ $name =~ ^[a-z0-9-]+$ ]] || { echo "update-lucide: odd icon name '$name'" >&2; exit 1; }
  [[ -f $package/icons/$name.svg ]] || { echo "update-lucide: tags.json names '$name' but the archive has no SVG" >&2; exit 1; }
  cp "$package/icons/$name.svg" "$stage/icons/"
  count=$((count + 1))
done < <(jq -r 'keys[]' "$package/tags.json" | LC_ALL=C sort)

printf '%s\n' "$version" > "$stage/VERSION"
printf 'url %s\nsha256 %s\nintegrity %s\n' "$url" "$archive_sha256" "$integrity" > "$stage/SOURCE"
(cd "$stage" && find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort | xargs sha256sum > SHA256SUMS)

# Swap only once the new set is complete, so a failure above leaves the old one.
if [[ -d $dest ]]; then
  mv "$dest" "$work/previous"
fi
mv "$stage" "$dest"

echo "update-lucide: vendored $count icons of Lucide $version into $dest"
echo "update-lucide: archive sha256 $archive_sha256"
