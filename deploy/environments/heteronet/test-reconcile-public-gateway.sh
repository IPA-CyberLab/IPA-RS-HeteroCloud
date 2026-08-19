#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$script_dir/reconcile-public-gateway.sh"

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

legacy_file="$tmp_dir/gateway-legacy.Caddyfile"
canonical_file="$tmp_dir/gateway-canonical.Caddyfile"
global_only_file="$tmp_dir/gateway-global-only.Caddyfile"

cat >"$legacy_file" <<'EOF'
{
  admin unix//run/heteronetwork-gateway/admin.sock|0660
  persist_config off
}

flow.heterocloud.mizuame.app {
  handle /v1/signal/* {
    reverse_proxy 10.250.0.4:8082
  }
  handle {
    reverse_proxy 10.250.0.4:8080
  }
}

163.220.236.51 {
  import /etc/heteronetwork/public-bootstrap-routes.Caddyfile
  handle {
    reverse_proxy 10.250.0.4:19088
  }
}
EOF

gateway_bootstrap_needs_reconcile "$legacy_file"
render_canonical_gateway_bootstrap "$canonical_file"

if grep -Eq '10\.250\.0\.4:(8080|8082|19088)' "$canonical_file"; then
  echo "legacy upstream survived canonicalization" >&2
  exit 1
fi
if grep -Fq 'flow.heterocloud.mizuame.app {' "$canonical_file"; then
  echo "legacy Flow site survived canonicalization" >&2
  exit 1
fi
if gateway_bootstrap_needs_reconcile "$canonical_file"; then
  echo "canonical bootstrap was not idempotent" >&2
  exit 1
fi

cat >"$global_only_file" <<'EOF'
{
  admin unix//run/heteronetwork-gateway/admin.sock|0660
  persist_config off
}
EOF
if gateway_bootstrap_needs_reconcile "$global_only_file"; then
  echo "global-only bootstrap should already be canonical" >&2
  exit 1
fi

gateway_json_is_canonical \
  '{"dial":"heterocloud-edge-proxy.envoy-gateway-system.svc.cluster.local:18081"}'
if gateway_json_is_canonical \
  '{"dial":"heterocloud-edge-proxy.envoy-gateway-system.svc.cluster.local:18081"}{"dial":"10.250.0.4:8080"}'; then
  echo "active configuration accepted duplicate legacy and Envoy routes" >&2
  exit 1
fi
if gateway_json_is_canonical '{"dial":"10.250.0.4:8082"}'; then
  echo "active configuration accepted a legacy-only route" >&2
  exit 1
fi

declare -A gateway_vpn_ip=(
  [a]=10.250.0.4
  [b]=10.250.0.5
  [c]=10.250.0.6
  [d]=10.250.0.10
)
readonly -a envoy_vpn_ips=(10.250.0.4 10.250.0.5 10.250.0.6 10.250.0.10)
readonly -a keycloak_vpn_ips=(10.250.0.4 10.250.0.5 10.250.0.6 10.250.0.8 10.250.0.10)

for gateway_id in a b c d; do
  gateway_file="$script_dir/public-gateway-$gateway_id.Caddyfile"
  local_vpn_ip=${gateway_vpn_ip[$gateway_id]}
  envoy_upstreams=$(grep -E '^\s*reverse_proxy (10\.250\.0\.[0-9]+:18082\s*){4}' "$gateway_file")
  keycloak_upstreams=$(grep -E '^\s*reverse_proxy (10\.250\.0\.[0-9]+:18079\s*){5}' "$gateway_file")

  grep -Fq 'http://:18082 {' "$gateway_file"
  grep -Fq "bind $local_vpn_ip" "$gateway_file"
  grep -Fq '@vpn remote_ip 10.250.0.0/16 127.0.0.0/8' "$gateway_file"
  grep -Fq 'health_uri {args[0]}' "$gateway_file"
  grep -Fq 'lb_try_duration 5s' "$gateway_file"
  if grep -Fq '127.0.0.1:18079' "$gateway_file"; then
    echo "gateway $gateway_id still pins Keycloak to localhost" >&2
    exit 1
  fi

  for vpn_ip in "${envoy_vpn_ips[@]}"; do
    grep -Fq "$vpn_ip:18082" <<<"$envoy_upstreams"
  done
  for vpn_ip in "${keycloak_vpn_ips[@]}"; do
    grep -Fq "$vpn_ip:18079" <<<"$keycloak_upstreams"
  done
  [[ $envoy_upstreams == *" $local_vpn_ip:18082 {" ]]
  [[ $keycloak_upstreams == *" $local_vpn_ip:18079 {" ]]
done

echo "public gateway reconciliation tests passed"
