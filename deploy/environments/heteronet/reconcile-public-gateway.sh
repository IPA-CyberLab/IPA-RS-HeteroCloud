#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "run as root" >&2
  exit 1
fi

gateway_id=${1:-}
case "$gateway_id" in
  a) expected_host=uc-k8sp1; public_ip=163.220.236.51 ;;
  b) expected_host=uc-k8sp2; public_ip=163.220.236.52 ;;
  c) expected_host=uc-k8s3p; public_ip=163.220.236.53 ;;
  d) expected_host=ichikawap1; public_ip=163.220.236.61 ;;
  *) echo "usage: $0 {a|b|c|d}" >&2; exit 2 ;;
esac

actual_host=$(hostname -s)
if [[ $actual_host != "$expected_host" ]]; then
  echo "gateway $gateway_id belongs on $expected_host, not $actual_host" >&2
  exit 1
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source_file="$script_dir/public-gateway-$gateway_id.Caddyfile"
target_file=/etc/heteronetwork/public-gateway-extra.Caddyfile
drop_in=/etc/systemd/system/heteronetwork-agent.service.d/40-public-gateway-extra.conf

install -d -o root -g root -m 0755 /etc/heteronetwork
install -d -o root -g root -m 0755 "$(dirname "$drop_in")"
install -o root -g root -m 0644 "$source_file" "$target_file.new"
mv -f "$target_file.new" "$target_file"

cat >"$drop_in.new" <<'EOF'
[Service]
Environment="HETERONETWORK_AGENT_PUBLIC_WEB_GATEWAY_EXTRA_CADDYFILE=/etc/heteronetwork/public-gateway-extra.Caddyfile"
EOF
chown root:root "$drop_in.new"
chmod 0644 "$drop_in.new"
mv -f "$drop_in.new" "$drop_in"

systemctl daemon-reload
systemctl restart heteronetwork-agent.service

for _ in $(seq 1 60); do
  if curl --fail --silent --show-error --insecure --max-time 3 \
    --resolve "flow.heterocloud.mizuame.app:443:$public_ip" \
    https://flow.heterocloud.mizuame.app/health/live >/dev/null; then
    echo "Flow gateway $gateway_id is ready on $public_ip"
    exit 0
  fi
  sleep 2
done

echo "Flow gateway $gateway_id did not become ready on $public_ip" >&2
exit 1
