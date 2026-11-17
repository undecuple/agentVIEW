#!/usr/bin/env bash
# CI: every commit must have the project's noreply identity and no attribution lines.
# Usage: check-history.sh <expected-email-regex> [rev-range] [extra-exact-author-email]
re="$1"; range="${2:-HEAD}"; extra_author="${3:-}"
fail=0
while read -r h; do
  ae=$(git log -1 --format='%ae' "$h"); ce=$(git log -1 --format='%ce' "$h")
  # Optional third argument: one extra exact e-mail accepted as AUTHOR only (the committer must still match).
  [[ "$ae" =~ $re || ( -n "$extra_author" && "$ae" == "$extra_author" ) ]] || { echo "history: bad author on $h"; fail=1; }
  [[ "$ce" =~ $re ]] || { echo "history: bad committer on $h"; fail=1; }
  if git log -1 --format=%B "$h" | grep -Eiq 'Co-Authored-By:|Claude-Session:|Generated with|claude\.ai/code'; then
    echo "history: attribution line in $h"; fail=1
  fi
  git ls-tree -r --name-only -z "$h" | ./scripts/scan-files.sh "$h" || { echo "history: bad file in $h"; fail=1; }
done < <(git rev-list "$range")
exit $fail
