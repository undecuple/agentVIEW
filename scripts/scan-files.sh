#!/usr/bin/env bash
# Shared by the pre-commit hook and CI. Reads NUL-separated paths on stdin.
# Usage: scan-files.sh <rev>   rev "" scans the index (staged blobs); otherwise scans that commit's blobs.
# Blocks secrets, keys, archives, installers, databases, compiled files and files over 1 MiB.
shopt -s nocasematch
rev="$1"
fail=0
name_re='\.(pem|key|p12|pfx|kdbx|zip|tar|gz|tgz|bz2|xz|7z|rar|iso|pkg|rap|rif|edat|sqlite|sqlite3|db|log|pyc|pyo|appimage|exe|msi|dmg|sprx|self|elf|bin)$|(^|/)(\.env(\..*)?|id_rsa.*|id_ed25519.*|id_ecdsa.*|credentials.*|.*token.*)$'
secret_re='BEGIN [A-Z ]*PRIVATE KEY|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|Bearer +[A-Za-z0-9._~+/=-]{20,}|(api[_-]?key|secret|password|token)["'"'"' ]*[:=] *["'"'"'][^"'"'"' ]{12,}'
while IFS= read -r -d '' f; do
  if [[ "$f" =~ $name_re ]]; then echo "scan: blocked file name/type: $f"; fail=1; continue; fi
  size=$(git cat-file -s "$rev:$f" 2>/dev/null || echo 0)
  if [ "$size" -gt 1048576 ]; then echo "scan: file over 1 MiB: $f"; fail=1; continue; fi
  if git show "$rev:$f" 2>/dev/null | grep -aEq "$secret_re"; then echo "scan: possible secret in: $f"; fail=1; fi
done
exit $fail
