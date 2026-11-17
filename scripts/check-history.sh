#!/usr/bin/env bash
# CI: every commit must have the project's noreply identity and no attribution lines.
# Usage: check-history.sh <expected-email-regex> [rev-range]
re="$1"; range="${2:-HEAD}"
fail=0
while read -r h; do
  ident=$(git log -1 --format='%ae|%ce' "$h")
  for e in ${ident//|/ }; do
    [[ "$e" =~ $re ]] || { echo "history: bad identity on $h"; fail=1; }
  done
  if git log -1 --format=%B "$h" | grep -Eiq 'Co-Authored-By:|Claude-Session:|Generated with|claude\.ai/code'; then
    echo "history: attribution line in $h"; fail=1
  fi
  git ls-tree -r --name-only -z "$h" | ./scripts/scan-files.sh "$h" || { echo "history: bad file in $h"; fail=1; }
done < <(git rev-list "$range")
exit $fail
