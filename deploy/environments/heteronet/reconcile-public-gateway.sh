#!/usr/bin/env bash
set -euo pipefail

readonly default_gateway_caddyfile=/etc/heteronetwork/gateway.Caddyfile
readonly default_extra_caddyfile=/etc/heteronetwork/public-gateway-extra.Caddyfile
readonly default_agent_drop_in=/etc/systemd/system/heteronetwork-agent.service.d/40-public-gateway-extra.conf
readonly default_caddy_admin_socket=/run/heteronetwork-gateway/admin.sock
readonly default_caddy_bin=/opt/heteronetwork/bin/caddy
readonly envoy_upstream=heterocloud-edge-proxy.envoy-gateway-system.svc.cluster.local:18081

trim_line() {
  local value=$1
  value=${value#"${value%%[![:space:]]*}"}
  value=${value%"${value##*[![:space:]]}"}
  printf '%s' "$value"
}

caddyfile_is_global_only() {
  local file=$1
  local line normalized

  while IFS= read -r line || [[ -n $line ]]; do
    normalized=$(trim_line "$line")
    case "$normalized" in
      ''|'{'|'}'|'persist_config off'|'admin unix/'*) ;;
      *) return 1 ;;
    esac
  done <"$file"
}

gateway_bootstrap_needs_reconcile() {
  local gateway_file=$1

  if caddyfile_is_global_only "$gateway_file"; then
    return 1
  fi
  grep -Eq 'flow\.heterocloud\.mizuame\.app|heterocloud_frontend|public-gateway-extra\.Caddyfile' \
    "$gateway_file"
}

render_canonical_gateway_bootstrap() {
  local output_file=$1

  # Caddy starts from disk before the Agent owns its runtime configuration.
  # Service routes belong only to the Agent-loaded extra Caddyfile.
  cat >"$output_file" <<EOF
{
	admin unix//run/heteronetwork-gateway/admin.sock|0660
	persist_config off
}
EOF
}

install_if_changed() {
  local source_file=$1
  local target_file=$2

  if [[ -f $target_file ]] && cmp -s "$source_file" "$target_file"; then
    return 10
  fi
  install -o root -g root -m 0644 "$source_file" "$target_file.new" \
    || return 1
  mv -f "$target_file.new" "$target_file" || return 1
}

reconcile_gateway_bootstrap() {
  local gateway_file=$1
  local caddy_bin=$2
  local candidate

  if ! gateway_bootstrap_needs_reconcile "$gateway_file"; then
    return 10
  fi

  candidate=$(mktemp "${gateway_file}.new.XXXXXX") || return 1
  render_canonical_gateway_bootstrap "$candidate" || return 1
  if ! "$caddy_bin" adapt --adapter caddyfile --config "$candidate" >/dev/null; then
    rm -f "$candidate"
    return 1
  fi
  if ! install -o root -g root -m 0644 "$candidate" "$gateway_file.new"; then
    rm -f "$candidate"
    return 1
  fi
  if ! mv -f "$gateway_file.new" "$gateway_file"; then
    rm -f "$candidate"
    return 1
  fi
  rm -f "$candidate"
}

gateway_json_is_canonical() {
  local active_config=$1

  grep -Fq "$envoy_upstream" <<<"$active_config" \
    && ! grep -Eq '10\.250\.0\.[0-9]+:(7880|8080|8082)' <<<"$active_config"
}

active_gateway_is_canonical() {
  local admin_socket=$1
  local active_config

  active_config=$(curl --fail --silent --show-error --unix-socket "$admin_socket" \
    http://localhost/config/) || return 1
  gateway_json_is_canonical "$active_config"
}

main() {
  if [[ $EUID -ne 0 ]]; then
    echo "run as root" >&2
    exit 1
  fi

  local gateway_id=${1:-}
  local expected_host public_ip
  case "$gateway_id" in
    a) expected_host=uc-k8sp1; public_ip=163.220.236.51 ;;
    b) expected_host=uc-k8sp2; public_ip=163.220.236.52 ;;
    c) expected_host=uc-k8s3p; public_ip=163.220.236.53 ;;
    d) expected_host=ichikawap1; public_ip=163.220.236.61 ;;
    *) echo "usage: $0 {a|b|c|d}" >&2; exit 2 ;;
  esac

  local actual_host
  actual_host=$(hostname -s)
  if [[ $actual_host != "$expected_host" ]]; then
    echo "gateway $gateway_id belongs on $expected_host, not $actual_host" >&2
    exit 1
  fi

  local script_dir source_file target_file drop_in gateway_caddyfile admin_socket caddy_bin
  script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  source_file="$script_dir/public-gateway-$gateway_id.Caddyfile"
  target_file=${HETEROCLOUD_GATEWAY_EXTRA_CADDYFILE:-$default_extra_caddyfile}
  drop_in=${HETEROCLOUD_GATEWAY_AGENT_DROP_IN:-$default_agent_drop_in}
  gateway_caddyfile=${HETEROCLOUD_GATEWAY_CADDYFILE:-$default_gateway_caddyfile}
  admin_socket=${HETEROCLOUD_GATEWAY_ADMIN_SOCKET:-$default_caddy_admin_socket}
  caddy_bin=${HETEROCLOUD_CADDY_BIN:-$default_caddy_bin}

  install -d -o root -g root -m 0755 "$(dirname "$target_file")"
  install -d -o root -g root -m 0755 "$(dirname "$drop_in")"

  local extra_changed=false drop_in_changed=false active_reload_needed=false install_result=0
  install_if_changed "$source_file" "$target_file" || install_result=$?
  case "$install_result" in
    0) extra_changed=true ;;
    10) ;;
    *) echo "failed to install $target_file" >&2; exit "$install_result" ;;
  esac

  local drop_in_candidate
  drop_in_candidate=$(mktemp "${drop_in}.new.XXXXXX")
  trap 'rm -f "$drop_in_candidate"' EXIT
  cat >"$drop_in_candidate" <<EOF
[Service]
Environment="HETERONETWORK_AGENT_PUBLIC_WEB_GATEWAY_EXTRA_CADDYFILE=$target_file"
EOF
  install_result=0
  install_if_changed "$drop_in_candidate" "$drop_in" || install_result=$?
  case "$install_result" in
    0) drop_in_changed=true ;;
    10) ;;
    *) echo "failed to install $drop_in" >&2; exit "$install_result" ;;
  esac

  local bootstrap_result=0
  reconcile_gateway_bootstrap "$gateway_caddyfile" "$caddy_bin" \
    || bootstrap_result=$?
  case "$bootstrap_result" in
    0) echo "Canonicalized the on-disk Caddy bootstrap configuration." ;;
    10) ;;
    *) echo "failed to canonicalize $gateway_caddyfile" >&2; exit "$bootstrap_result" ;;
  esac

  if ! active_gateway_is_canonical "$admin_socket"; then
    active_reload_needed=true
  fi
  if [[ $drop_in_changed == true ]]; then
    systemctl daemon-reload
  fi
  if [[ $extra_changed == true || $drop_in_changed == true || $active_reload_needed == true ]]; then
    systemctl restart heteronetwork-agent.service
  fi

  local consecutive_successes=0
  for _ in $(seq 1 60); do
    if active_gateway_is_canonical "$admin_socket" \
      && curl --fail --silent --show-error --insecure --max-time 3 \
        --resolve "flow.heterocloud.mizuame.app:443:$public_ip" \
        https://flow.heterocloud.mizuame.app/health/live >/dev/null; then
      consecutive_successes=$((consecutive_successes + 1))
      if ((consecutive_successes >= 5)); then
        echo "Flow gateway $gateway_id is ready on $public_ip with only the Envoy route active."
        exit 0
      fi
    else
      consecutive_successes=0
    fi
    sleep 2
  done

  echo "Flow gateway $gateway_id did not sustain a canonical healthy route on $public_ip" >&2
  exit 1
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  main "$@"
fi
