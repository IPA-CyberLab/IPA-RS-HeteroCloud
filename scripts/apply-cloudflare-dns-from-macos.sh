#!/usr/bin/env bash

set -euo pipefail

readonly DOMAIN="heterocloud.mizuame.app"
readonly CLOUDFLARE_ZONE="mizuame.app"
readonly RELEASE_VERSION="v0.1.3"
readonly DEFAULT_REMOTE_HOST="mizuame@163.220.236.51"
readonly DEFAULT_TOKEN_FILE="$HOME/.config/heterocloud/cloudflare-token"
readonly KUBECONFIG_PATH="/etc/kubernetes/admin.conf"

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

cloudflare_request() {
  curl --fail --silent --show-error --header "@$AUTH_FILE" "$@"
}

migrate_legacy_records() {
  local label fqdn response record_ids record_id

  printf 'Migrating any manually-created HeteroCloud A records...\n'
  for label in \
    cloud-a cloud-b cloud-c \
    flow-a flow-b flow-c \
    rtc-a rtc-b rtc-c \
    turn-a turn-b turn-c
  do
    fqdn="$label.$DOMAIN"
    response="$(cloudflare_request \
      "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records?type=A&name=$fqdn")"
    record_ids="$(printf '%s' "$response" | jq -r \
      'if .success then .result[].id else error("record lookup failed") end')"
    for record_id in $record_ids; do
      cloudflare_request --request DELETE \
        "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records/$record_id" \
        | jq -e '.success == true' >/dev/null
      printf 'Removed unmanaged record %s for ExternalDNS adoption.\n' "$fqdn"
    done
  done
}

remote_main() {
  local token_name="$1"
  local script_name="$2"
  local asset base_url zone_response

  TOKEN_FILE="$HOME/$token_name"
  SELF_FILE="$HOME/$script_name"
  AUTH_FILE=""
  WORK_DIR=""

  cleanup_remote() {
    [ -z "$AUTH_FILE" ] || rm -f -- "$AUTH_FILE"
    [ -z "$WORK_DIR" ] || rm -rf -- "$WORK_DIR"
    rm -f -- "$TOKEN_FILE" "$SELF_FILE"
  }
  trap cleanup_remote EXIT

  for command_name in curl jq sha256sum tar sudo kubectl helm; do
    require_command "$command_name"
  done
  [ -f "$TOKEN_FILE" ] && [ ! -L "$TOKEN_FILE" ] \
    || fail "the transferred token is not a regular file"
  chmod 600 "$TOKEN_FILE"

  AUTH_FILE="$(mktemp /tmp/heterocloud-cloudflare-auth.XXXXXX)"
  WORK_DIR="$(mktemp -d /tmp/heterocloud-dns-bootstrap.XXXXXX)"
  chmod 600 "$AUTH_FILE"
  {
    printf 'Authorization: Bearer '
    cat "$TOKEN_FILE"
    printf '\n'
  } >"$AUTH_FILE"

  zone_response="$(cloudflare_request \
    "https://api.cloudflare.com/client/v4/zones?name=$CLOUDFLARE_ZONE&status=active")"
  ZONE_ID="$(printf '%s' "$zone_response" | jq -er \
    'if .success and (.result | length == 1) then .result[0].id else error("zone lookup failed") end')"
  printf 'Cloudflare zone verified: %s (%s)\n' "$CLOUDFLARE_ZONE" "$ZONE_ID"

  asset="heterocloud-$RELEASE_VERSION-linux-x64.tar.gz"
  base_url="https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/download/$RELEASE_VERSION"
  curl --fail --silent --show-error --location \
    --output "$WORK_DIR/$asset" "$base_url/$asset"
  curl --fail --silent --show-error --location \
    --output "$WORK_DIR/$asset.sha256" "$base_url/$asset.sha256"
  (cd "$WORK_DIR" && sha256sum --check "$asset.sha256")
  tar -xzf "$WORK_DIR/$asset" -C "$WORK_DIR"

  sudo -v
  sudo install -m 755 "$WORK_DIR/heterocloud" /usr/local/bin/heterocloud

  if ! sudo kubectl --kubeconfig "$KUBECONFIG_PATH" \
    --namespace heterocloud-dns get deployment heterocloud-dns \
    >/dev/null 2>&1
  then
    migrate_legacy_records
  fi

  sudo /usr/local/bin/heterocloud dns reconcile \
    --domain "$DOMAIN" \
    --provider cloudflare \
    --credential-file "CF_API_TOKEN=$TOKEN_FILE" \
    --provider-arg="--zone-id-filter=$ZONE_ID" \
    --public-ip 163.220.236.51 \
    --public-ip 163.220.236.52 \
    --public-ip 163.220.236.53 \
    --kubeconfig "$KUBECONFIG_PATH"

  printf 'Cloudflare DNS reconciliation completed for %s.\n' "$DOMAIN"
}

local_main() {
  local remote_host token_file ssh_key script_path run_id
  local remote_token remote_script remote_cleanup_pending
  local -a ssh_options

  remote_host="${HETEROCLOUD_DNS_REMOTE_HOST:-$DEFAULT_REMOTE_HOST}"
  token_file="${HETEROCLOUD_CF_TOKEN_FILE:-$DEFAULT_TOKEN_FILE}"
  ssh_key="${HETEROCLOUD_SSH_KEY:-}"
  script_path="$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")"

  for command_name in scp ssh; do
    require_command "$command_name"
  done
  [ -s "$token_file" ] && [ ! -L "$token_file" ] \
    || fail "token file is missing, empty, or a symbolic link: $token_file"
  [ "$(wc -l <"$token_file" | tr -d ' ')" = "0" ] \
    || fail "token file must not contain line breaks"
  chmod 600 "$token_file"

  ssh_options=(-o ServerAliveInterval=30)
  if [ -n "$ssh_key" ]; then
    ssh_options+=( -i "$ssh_key" )
  fi

  run_id="$(date +%s)-$$"
  remote_token=".heterocloud-cloudflare-token-$run_id"
  remote_script=".heterocloud-dns-bootstrap-$run_id.sh"
  remote_cleanup_pending=1

  cleanup_local() {
    if [ "$remote_cleanup_pending" = "1" ]; then
      ssh "${ssh_options[@]}" -o BatchMode=yes "$remote_host" \
        "rm -f -- '$remote_token' '$remote_script'" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup_local EXIT

  printf 'Transferring credentials and bootstrap script to %s...\n' "$remote_host"
  scp "${ssh_options[@]}" -p "$token_file" "$remote_host:$remote_token"
  scp "${ssh_options[@]}" -p "$script_path" "$remote_host:$remote_script"

  ssh "${ssh_options[@]}" -t "$remote_host" \
    "bash '$remote_script' --remote '$remote_token' '$remote_script'"
  remote_cleanup_pending=0
  trap - EXIT
}

if [ "${1:-}" = "--remote" ]; then
  [ "$#" = "3" ] || fail "invalid remote bootstrap invocation"
  remote_main "$2" "$3"
else
  [ "$#" = "0" ] || fail "this script does not accept positional arguments"
  local_main
fi
