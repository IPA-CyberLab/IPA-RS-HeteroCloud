#!/bin/sh
# Wait for the current Flash revision before Coder marks a workspace running.
# The service response can contain environment secrets: keep it in pipes and
# shell memory, and print only the whitelisted status fields below.
set -eu
umask 077

bridge=${HETEROCLOUD_CODER_BRIDGE:?missing provisioner bridge URL}
account=${HETEROCLOUD_CODER_ACCOUNT_ID:?missing account ID}
service=${HETEROCLOUD_CODER_SERVICE_ID:?missing service ID}
timeout=${HETEROCLOUD_CODER_WAIT_SECONDS:-420}
interval=${HETEROCLOUD_CODER_POLL_SECONDS:-5}
capacity_seconds=${HETEROCLOUD_CODER_CAPACITY_WAIT_SECONDS:-0}
capacity_request=${HETEROCLOUD_CODER_CAPACITY_WAIT_ID:-}

case "$bridge" in http://*|https://*) ;; *) echo 'Invalid Flash provisioner endpoint.' >&2; exit 1 ;; esac
for id in "$account" "$service"; do
  case "$id" in *[!0-9a-fA-F-]*|'') echo 'Invalid Flash account or service ID.' >&2; exit 1 ;; esac
  [ "${#id}" -eq 36 ] || { echo 'Invalid Flash account or service ID.' >&2; exit 1; }
done
for n in "$timeout" "$interval"; do
  case "$n" in *[!0-9]*|'') echo 'Invalid workspace wait setting.' >&2; exit 1 ;; esac
  [ "$n" -gt 0 ] && [ "$n" -le 1800 ] || { echo 'Invalid workspace wait setting.' >&2; exit 1; }
done
case "$capacity_seconds" in *[!0-9]*|'') echo 'Invalid capacity wait setting.' >&2; exit 1 ;; esac
[ "$capacity_seconds" -le 900 ] || { echo 'Invalid capacity wait setting.' >&2; exit 1; }
if [ -n "$capacity_request" ]; then
  case "$capacity_request" in *[!0-9a-fA-F-]*) echo 'Invalid capacity request ID.' >&2; exit 1 ;; esac
  [ "${#capacity_request}" -eq 36 ] || { echo 'Invalid capacity request ID.' >&2; exit 1; }
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
endpoint="${bridge%/}/api/v1/organizations/$account/flash/services/$service"
cancel_capacity_wait() {
  if [ -n "$capacity_request" ]; then
    curl -q -fsS --connect-timeout 2 --max-time 5 -X PUT \
      -H 'Content-Type: application/json' \
      --data "{\"cancel_capacity_wait_id\":\"$capacity_request\"}" "$endpoint" >/dev/null 2>&1 || :
  fi
}
trap 'cancel_capacity_wait; exit 1' HUP INT TERM
deadline=$(( $(date +%s) + timeout ))
if [ -n "$capacity_request" ]; then
  deadline=$(( deadline + capacity_seconds + 30 ))
fi
last_problem=''
problem_count=0
last_problem_at=0
reported_problem=''
reported_capacity=''
printf '%s\n' 'Flash の起動と永続ディスクの準備を待っています。'

describe_problem() {
  case "$1" in
    insufficient_replicated_storage) printf '%s' 'Flash: 永続ディスクを複製する空き容量が不足しています。' ;;
    insufficient_disk_capacity) printf '%s' 'Flash: コンテナのディスク空き容量が不足しています。' ;;
    insufficient_compute_capacity) printf '%s' 'Flash: CPU またはメモリの空き容量が不足しています。' ;;
    insufficient_pod_capacity) printf '%s' 'Flash: コンテナの空き枠が不足しています。' ;;
    insufficient_inodes) printf '%s' 'Flash: ホストのファイルシステム容量が不足しています。' ;;
    insufficient_topology_capacity|no_eligible_node) printf '%s' 'Flash: 配置できるホストの空きを待っています。' ;;
    gpu_unavailable) printf '%s' 'Flash: GPU の空きを待っています。' ;;
    capacity_metrics_unavailable) printf '%s' 'Flash: 空き容量の取得を再試行しています。' ;;
    authorization_revoked|allocation_denied) printf '%s' 'Flash: 権限または割り当て上限が変更されました。' ;;
    storage_replica_policy_unavailable) printf '%s' 'Flash: 永続ディスクの複製設定を確認できません。' ;;
    image_unavailable) printf '%s' 'Flash: ワークスペースのイメージが見つかりません。テンプレートを更新してください。' ;;
  esac
}

while [ "$(date +%s)" -lt "$deadline" ]; do
  # No response is written to disk or used as a command-line argument.
  encoded=$(curl -fsS --connect-timeout 3 --max-time 10 --max-filesize 4194304 "$endpoint" 2>/dev/null | base64 | tr -d '\n') || encoded=''
  if [ -n "$encoded" ]; then
    expression="[for s in [jsondecode(base64decode(\"$encoded\"))] : format(\"%d|%d|%s|%d|%s|%s|%s|%s|%s\", try(s.generation, 0), try(s.status.status.observed_generation, 0), try(s.status.status.phase, \"\"), try(s.status.status.ready_replicas, 0), try(s.status.status.diagnostics[0].code, \"\"), tostring(try(strcontains(lower(s.status.status.message), \"waiting for image inspection\") && strcontains(lower(s.status.status.message), \"not found\"), false)), try(s.status.capacity_wait.id, \"\"), try(s.status.capacity_wait.state, \"\"), coalesce(try(s.status.capacity_wait.reason, null), \"none\"))][0]"
    result=$(printf '%s\n' "$expression" | TF_DATA_DIR="$tmp/.terraform" TF_WORKSPACE=default TF_LOG=off TF_LOG_CORE=off TF_LOG_PROVIDER=off TF_LOG_PATH=/dev/null terraform -chdir="$tmp" console -no-color 2>/dev/null) || result=''
    unset encoded expression
    case "$result" in
      \"*\") result=${result#\"}; result=${result%\"} ;;
      *) result='' ;;
    esac
    if [ -n "$result" ]; then
      IFS='|' read -r generation observed phase ready problem image_missing wait_id wait_state wait_reason <<EOF_STATUS
$result
EOF_STATUS
      if [ -n "$capacity_request" ] && [ "$wait_id" != "$capacity_request" ]; then
        printf '%s\n' 'Flash: 容量待ちの起動要求が更新されました。' >&2
        exit 1
      fi
      if [ -n "$capacity_request" ]; then
        case "$wait_state" in
          waiting|starting)
            if [ "$reported_capacity" != "$wait_state:$wait_reason" ]; then
              message=$(describe_problem "$wait_reason")
              if [ -n "$message" ]; then
                printf '%s\n' "$message"
              elif [ "$wait_state" = waiting ]; then
                printf '%s\n' 'Flash: 空き容量を待っています。'
              else
                printf '%s\n' 'Flash: コンテナの起動を待っています。'
              fi
              reported_capacity="$wait_state:$wait_reason"
            fi
            sleep "$interval"
            continue ;;
          ready) ;;
          expired) printf '%s\n' 'Flash: 起動要求の期限が切れました。' >&2; exit 1 ;;
          rejected)
            message=$(describe_problem "$wait_reason")
            printf '%s\n' "${message:-Flash: 起動要求を完了できませんでした。}" >&2
            exit 1 ;;
          *) printf '%s\n' 'Flash: 起動要求が取り消されました。' >&2; exit 1 ;;
        esac
      fi
      case "$generation:$observed:$ready" in
        *[!0-9:]*|::*|:*|*:) result='' ;;
      esac
      if [ -n "$result" ] && [ "$generation" -gt 0 ] && [ "$observed" -ge "$generation" ]; then
        if [ "$phase" = ready ] && [ "$ready" -ge 1 ]; then
          printf '%s\n' 'Flash のコンテナが起動しました。'
          exit 0
        fi
        if [ "$image_missing" = true ]; then problem=image_unavailable; fi
        case "$problem" in
          insufficient_replicated_storage|insufficient_disk_capacity|insufficient_compute_capacity|storage_replica_policy_unavailable|image_unavailable)
            if [ "$problem" = "$last_problem" ]; then problem_count=$((problem_count + 1)); else problem_count=1; fi
            last_problem=$problem
            last_problem_at=$(date +%s)
            if [ "$problem_count" -ge 3 ] && [ "$reported_problem" != "$problem" ]; then
              describe_problem "$problem"
              printf '%s\n' ' 復旧を待っています。'
              reported_problem=$problem
            fi ;;
          *) last_problem=''; problem_count=0; last_problem_at=0; reported_problem='' ;;
        esac
      fi
    fi
  fi
  sleep "$interval"
done
cancel_capacity_wait
if [ "$problem_count" -ge 3 ] && [ $(( $(date +%s) - last_problem_at )) -le $(( interval * 2 + 10 )) ]; then
  describe_problem "$last_problem" >&2
  printf '\n' >&2
  exit 1
fi
printf '%s\n' 'Flash の起動を確認できませんでした。サービスの状態と容量を確認して再試行してください。' >&2
exit 1
