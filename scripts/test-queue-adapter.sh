#!/usr/bin/env bash
# Checks scripts/adapters/lockdir-queue.sh against a fixture lock folder (no app needed). Needs jq.
set -euo pipefail
cd "$(dirname "$0")/.."
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT; mkdir "$t/locks"; now=$(date +%s)
printf 'start %s 0 10 proj-a 111\nstart %s 1 20 proj-b 222\nend %s 222\nstart %s 0 5 proj-c 333\n' "$((now-900))" "$((now-800))" "$((now-700))" "$((now-100))" > "$t/locks/gpu.history"
printf 'maintenance (since 10:00)\nexpires %s\nowner someone\n' "$((now+600))" > "$t/locks/gpu.reserved"
printf 'proj-d\t30\tprobe one\nproj-e\t5\tcheck\n' > "$t/locks/gpu.order"; printf '4242\n' > "$t/locks/gpu.queue"
bash scripts/adapters/lockdir-queue.sh "$t/locks" gpu "$t/out.jsonl"
[ "$(stat -c %a "$t/out.jsonl")" = 600 ] || { echo "mode"; exit 1; }
r=$(jq -r 'select(.ev=="running") | .who+":"+.slot' "$t/out.jsonl" | sort | tr '\n' ' ')
[ "$r" = "proj-c:0 " ] || { echo "running should be only proj-c in slot 0 (proj-a superseded, proj-b ended): got '$r'"; exit 1; }
[ "$(jq -r 'select(.ev=="reserved") | .who' "$t/out.jsonl")" = someone ] || { echo "reserved"; exit 1; }
[ "$(jq -r 'select(.ev=="queued" and .pos<10) | .who' "$t/out.jsonl" | tr '\n' ' ')" = "proj-d proj-e " ] || { echo "posted order"; exit 1; }
grep -q '"pid-4242"' "$t/out.jsonl" || { echo "queue pid"; exit 1; }
printf 'expires %s\n' "$((now-5))" > "$t/locks/gpu.reserved"; bash scripts/adapters/lockdir-queue.sh "$t/locks" gpu "$t/out.jsonl"
! grep -q '"reserved"' "$t/out.jsonl" || { echo "expired reservation should be dropped"; exit 1; }
! bash scripts/adapters/lockdir-queue.sh "$t/locks" "../x" "$t/o2" 2>/dev/null || { echo "bad name accepted"; exit 1; }
echo "Queue adapter: PASS"
