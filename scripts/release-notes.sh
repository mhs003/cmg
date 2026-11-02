#!/usr/bin/env bash
# Work out the next version from Conventional Commits and update VERSION plus
# CHANGELOG.md. Used by the release workflow; runnable by hand.
#
# Usage: scripts/release-notes.sh <current-version> <git-log-range>
#   e.g. scripts/release-notes.sh 0.1.0 v0.1.0..HEAD
# Prints the new version on stdout. Exit 3 means "nothing to release".
set -euo pipefail

VERSION_FILE=${VERSION_FILE:-VERSION}
CHANGELOG_FILE=${CHANGELOG_FILE:-CHANGELOG.md}

current=$1
range=$2

# One record per commit: "subject<TAB>body", newlines flattened.
commits=$(git log --no-merges --pretty=format:'%s%x1f%b%x1e' "$range" 2>/dev/null || true)

bump=patch
breaking_entries=""
feat_entries=""
fix_entries=""
other_entries=""
releasable=0

append() {
  local var=$1 text=$2
  printf -v "$var" '%s- %s\n' "${!var}" "$text"
}

while IFS= read -r -d $'\x1e' record; do
  [ -z "$record" ] && continue
  subject=${record%%$'\x1f'*}
  body=${record#*$'\x1f'}
  subject=${subject%$'\n'}

  # Skip the releases this workflow itself creates.
  case "$subject" in
    "chore(release):"*) continue ;;
  esac

  type=$(printf '%s' "$subject" | sed -n 's/^\([a-zA-Z]*\)\(([^)]*)\)\?\(!\)\?: .*/\1/p')
  scope=$(printf '%s' "$subject" | sed -n 's/^[a-zA-Z]*\(([^)]*)\)\?[!]\?: .*/\1/p')
  desc=$(printf '%s' "$subject" | sed -n 's/^[a-zA-Z]*\(([^)]*)\)\?[!]\?: //p')
  [ -z "$desc" ] && desc="$subject"

  label=$desc
  if [ -n "$scope" ]; then
    scope=${scope#(}; scope=${scope%)}
    label="**${scope}:** ${desc}"
  fi

  breaking=0
  case "$subject" in *"!:"*) breaking=1 ;; esac
  case "$body" in *"BREAKING CHANGE"*) breaking=1 ;; esac

  if [ "$breaking" = 1 ]; then
    append breaking_entries "$label"
    bump=minor
    releasable=1
    continue
  fi

  case "$type" in
    feat) append feat_entries "$label"; [ "$bump" = patch ] && bump=minor; releasable=1 ;;
    fix|perf|revert) append fix_entries "$label"; releasable=1 ;;
    refactor) append fix_entries "$label"; releasable=1 ;;
    *) append other_entries "$label" ;;
  esac
done <<< "$commits"

if [ "$releasable" = 0 ]; then
  echo "no releasable commits in range $range" >&2
  exit 3
fi

IFS=. read -r major minor patch <<< "$current"
case "$bump" in
  minor) minor=$((minor + 1)); patch=0 ;;
  *)     patch=$((patch + 1)) ;;
esac
next="$major.$minor.$patch"

date=$(date -u +%Y-%m-%d)
section="## [$next] - $date"$'\n\n'
[ -n "$breaking_entries" ] && section+="### Breaking Changes"$'\n\n'"$breaking_entries"$'\n'
[ -n "$feat_entries" ]     && section+="### Added"$'\n\n'"$feat_entries"$'\n'
[ -n "$fix_entries" ]      && section+="### Fixed"$'\n\n'"$fix_entries"$'\n'
[ -n "$other_entries" ]    && section+="### Other"$'\n\n'"$other_entries"$'\n'

printf '%s\n' "$next" > "$VERSION_FILE"

# Insert the section after the "## [Unreleased]" heading and its placeholder.
awk -v sec="$section" '
  { print }
  /^## \[Unreleased\]/ { print ""; printf "%s", sec; next }
' "$CHANGELOG_FILE" > "$CHANGELOG_FILE.tmp"
mv "$CHANGELOG_FILE.tmp" "$CHANGELOG_FILE"

echo "$next"
