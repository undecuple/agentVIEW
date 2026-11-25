#!/usr/bin/env bash
# Turns a lock folder into a queue file that agentVIEW's "Queues" card shows. Read-only for the lock folder; it only writes OUT_FILE (atomically).
# usage: lockdir-queue.sh LOCK_DIR NAME OUT_FILE      (OUT_FILE = <app data>/queues/NAME.jsonl)
# Reads, if present, in LOCK_DIR:
#   NAME.history   lines "start EPOCH SLOT EST_MIN PROJECT PID" and "end EPOCH PID"  -> running (the newest start per slot that has not ended)
#   NAME.reserved  first line = reason; "expires EPOCH"; "owner NAME"                -> reserved while it has not expired
#   NAME.order     tab-separated "PROJECT<TAB>EST_MIN<TAB>NOTE" per line, in order     -> waiting (posted queue)
#   NAME.queue     one waiting pid per line                                            -> waiting (details unknown)
# Run it from your lock script on enqueue, start and end, or on a timer. Needs jq and awk.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: $0 LOCK_DIR NAME OUT_FILE" >&2; exit 2; }
d=$1; n=$2; out=$3; now=$(date +%s)
case "$n" in *[!A-Za-z0-9._-]*|"") echo "bad name" >&2; exit 2;; esac
[ -d "$d" ] || { echo "no such folder" >&2; exit 2; }
tmp=$(mktemp "$out.XXXXXX"); trap 'rm -f "$tmp"' EXIT
{
  if [ -f "$d/$n.history" ]; then
    tail -n 400 "$d/$n.history" | awk '
      $1=="start"{k++; line[k]=$0; slot[k]=$3; last[$6]=k; next}
      $1=="end"{ if(($3 in last)) ended[last[$3]]=1 }
      END{ for(i=k;i>=1;i--){ if(slot[i] in seen) continue; seen[slot[i]]=1; if(!(i in ended)) print line[i] } }' |
    while read -r _ t slot est proj pid; do
      jq -nc --argjson t "${t:-0}" --arg id "run-$pid" --arg who "$proj" --argjson est "${est:-0}" --arg slot "$slot" '{t:$t,id:$id,ev:"running",who:$who,est_min:$est,slot:$slot}' 2>/dev/null || true
    done
  fi
  if [ -f "$d/$n.reserved" ]; then
    reason=$(head -n1 "$d/$n.reserved"); exp=$(awk '$1=="expires"{print $2}' "$d/$n.reserved"); own=$(awk '$1=="owner"{print $2}' "$d/$n.reserved")
    if [ -n "${exp:-}" ] && [ "$exp" -gt "$now" ] 2>/dev/null; then
      jq -nc --argjson t "$now" --arg who "${own:-}" --arg note "$reason" --argjson until "$exp" '{t:$t,id:"reserve",ev:"reserved",who:$who,note:$note,until:$until}'
    fi
  fi
  if [ -f "$d/$n.order" ]; then
    i=0; while IFS=$'\t' read -r proj est note; do i=$((i+1)); [ -n "$proj" ] || continue
      jq -nc --argjson t "$now" --arg id "posted-$i" --arg who "$proj" --argjson est "${est:-0}" --argjson pos "$i" --arg note "${note:-}" '{t:$t,id:$id,ev:"queued",who:$who,est_min:$est,pos:$pos,note:$note}' 2>/dev/null || true
    done < "$d/$n.order"
  fi
  if [ -f "$d/$n.queue" ]; then
    while read -r pid; do case "$pid" in ""|*[!0-9]*) continue;; esac
      jq -nc --argjson t "$now" --arg id "pid-$pid" --arg who "pid $pid" '{t:$t,id:$id,ev:"queued",who:$who,pos:1000}'
    done < "$d/$n.queue"
  fi
} > "$tmp"
chmod 600 "$tmp"; mv -f "$tmp" "$out"; trap - EXIT
