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

case "$bridge" in http://*|https://*) ;; *) echo 'Invalid Flash provisioner endpoint.' >&2; exit 1 ;; esac
for id in "$account" "$service"; do
  case "$id" in *[!0-9a-fA-F-]*|'') echo 'Invalid Flash account or service ID.' >&2; exit 1 ;; esac
  [ "${#id}" -eq 36 ] || { echo 'Invalid Flash account or service ID.' >&2; exit 1; }
done
for n in "$timeout" "$interval"; do
  case "$n" in *[!0-9]*|'') echo 'Invalid workspace wait setting.' >&2; exit 1 ;; esac
  [ "$n" -gt 0 ] && [ "$n" -le 1800 ] || { echo 'Invalid workspace wait setting.' >&2; exit 1; }
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' HUP INT TERM
endpoint="${bridge%/}/api/v1/organizations/$account/flash/services/$service"
deadline=$(( $(date +%s) + timeout ))
last_problem=''
problem_count=0
printf '%s\n' 'Flash の起動と永続ディスクの準備を待っています。'

while [ "$(date +%s)" -lt "$deadline" ]; do
  # No response is written to disk or used as a command-line argument.
  encoded=$(curl -fsS --connect-timeout 3 --max-time 10 --max-filesize 4194304 "$endpoint" 2>/dev/null | base64 | tr -d '\n') || encoded=''
  if [ -n "$encoded" ]; then
    expression="[for s in [jsondecode(base64decode(\"$encoded\"))] : format(\"%d|%d|%s|%d|%s|%s\", try(s.generation, 0), try(s.status.status.observed_generation, 0), try(s.status.status.phase, \"\"), try(s.status.status.ready_replicas, 0), try(s.status.status.diagnostics[0].code, \"\"), tostring(try(strcontains(lower(s.status.status.message), \"waiting for image inspection\") && strcontains(lower(s.status.status.message), \"not found\"), false)))][0]"
    result=$(printf '%s\n' "$expression" | TF_DATA_DIR="$tmp/.terraform" TF_WORKSPACE=default TF_LOG=off TF_LOG_CORE=off TF_LOG_PROVIDER=off TF_LOG_PATH=/dev/null terraform -chdir="$tmp" console -no-color 2>/dev/null) || result=''
    unset encoded expression
    case "$result" in
      \"*\") result=${result#\"}; result=${result%\"} ;;
      *) result='' ;;
    esac
    if [ -n "$result" ]; then
      IFS='|' read -r generation observed phase ready problem image_missing <<EOF_STATUS
$result
EOF_STATUS
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
            if [ "$problem_count" -ge 3 ]; then
              case "$problem" in
                insufficient_replicated_storage) echo 'Flash: 永続ディスクを複製する空き容量が不足しています。' >&2 ;;
                insufficient_disk_capacity) echo 'Flash: コンテナのディスク空き容量が不足しています。' >&2 ;;
                insufficient_compute_capacity) echo 'Flash: CPU またはメモリの空き容量が不足しています。' >&2 ;;
                storage_replica_policy_unavailable) echo 'Flash: 永続ディスクの複製設定を確認できません。' >&2 ;;
                image_unavailable) echo 'Flash: ワークスペースのイメージが見つかりません。テンプレートを更新してください。' >&2 ;;
              esac
              exit 1
            fi ;;
          *) last_problem=''; problem_count=0 ;;
        esac
      fi
    fi
  fi
  sleep "$interval"
done
printf '%s\n' 'Flash の起動を確認できませんでした。サービスの状態と容量を確認して再試行してください。' >&2
exit 1
