#!/usr/bin/env bash
set -euo pipefail

umask 077

readonly server="${HETEROCLOUD_KEYCLOAK_SERVER:-http://127.0.0.1:18080}"
readonly realm="${HETEROCLOUD_KEYCLOAK_REALM:-heterocloud}"
readonly client_id="${HETEROCLOUD_KEYCLOAK_CLIENT_ID:-heterocloud-web}"
readonly public_origin="${HETEROCLOUD_PUBLIC_ORIGIN:-https://heterocloud.mizuame.app}"
readonly identity_origin="${HETEROCLOUD_IDENTITY_ORIGIN:-${public_origin}/id}"
readonly callback_url="${HETEROCLOUD_OIDC_CALLBACK_URL:-${public_origin}/api/v1/auth/oidc/callback}"
readonly owner_origin="${HETEROCLOUD_OWNER_ORIGIN:-http://owner.heteronetwork.internal:21443}"
readonly owner_callback_url="${HETEROCLOUD_OWNER_OIDC_CALLBACK_URL:-${owner_origin}/api/v1/auth/oidc/callback}"
readonly admin_password_file="${HETEROCLOUD_KEYCLOAK_ADMIN_PASSWORD_FILE:-/etc/heteronetwork/keycloak/bootstrap-admin.password}"
readonly client_secret_file="${HETEROCLOUD_OIDC_CLIENT_SECRET_FILE:-/etc/heterocloud/oidc/client-secret}"
readonly kcadm="${HETEROCLOUD_KCADM:-/opt/heteronetwork/keycloak/bin/kcadm.sh}"

fail() {
  printf 'reconcile-keycloak: %s\n' "$*" >&2
  exit 1
}

[[ "$(id -u)" == 0 ]] || fail "must run as root"
[[ "$realm" =~ ^[A-Za-z0-9._-]+$ ]] || fail "invalid realm"
[[ "$client_id" =~ ^[A-Za-z0-9._-]+$ ]] || fail "invalid client ID"
[[ "$public_origin" =~ ^https://[^/]+$ ]] || fail "public origin must be an HTTPS origin"
[[ "$identity_origin" =~ ^https://[^/]+/id$ ]] \
  || fail "identity origin must be an HTTPS origin ending in /id"
[[ "$callback_url" == "${public_origin}/api/v1/auth/oidc/callback" ]] \
  || fail "callback URL must use the canonical public origin"
[[ "$owner_origin" =~ ^https?://owner\.heteronetwork\.internal(:[0-9]+)?$ ]] \
  || fail "owner console origin must use the private HeteroNetwork DNS name"
[[ "$owner_callback_url" == "${owner_origin}/api/v1/auth/oidc/callback" ]] \
  || fail "owner callback URL must use the private owner console origin"
[[ -x "$kcadm" ]] || fail "kcadm is unavailable"
[[ -f "$admin_password_file" && ! -L "$admin_password_file" ]] \
  || fail "bootstrap administrator password is unavailable"
command -v jq >/dev/null || fail "jq is unavailable"
command -v openssl >/dev/null || fail "openssl is unavailable"

secret_dir="$(dirname "$client_secret_file")"
install -d -o root -g root -m 0700 "$secret_dir"
if [[ -e "$client_secret_file" ]]; then
  [[ -f "$client_secret_file" && ! -L "$client_secret_file" ]] \
    || fail "client secret path is unsafe"
  chown root:root "$client_secret_file"
  chmod 0600 "$client_secret_file"
fi

work_dir="$(mktemp -d /run/heterocloud-keycloak.XXXXXX)"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
readonly config_file="$work_dir/kcadm.config"
readonly realm_file="$work_dir/realm.json"
readonly clients_file="$work_dir/clients.json"
readonly client_file="$work_dir/client.json"

admin_password="$(tr -d '\r\n' <"$admin_password_file")"
[[ -n "$admin_password" ]] || fail "bootstrap administrator password is empty"
KC_CLI_PASSWORD="$admin_password" "$kcadm" config credentials \
  --config "$config_file" \
  --server "$server" \
  --realm master \
  --user admin </dev/null >/dev/null
unset admin_password

jq -n \
  --arg realm "$realm" \
  --arg frontend_url "$identity_origin" \
  '{
    realm: $realm,
    enabled: true,
    displayName: "HeteroCloud",
    sslRequired: "external",
    registrationAllowed: true,
    registrationEmailAsUsername: true,
    rememberMe: true,
    verifyEmail: false,
    loginWithEmailAllowed: true,
    duplicateEmailsAllowed: false,
    resetPasswordAllowed: false,
    editUsernameAllowed: false,
    bruteForceProtected: true,
    permanentLockout: false,
    failureFactor: 5,
    waitIncrementSeconds: 60,
    maxFailureWaitSeconds: 900,
    maxDeltaTimeSeconds: 43200,
    accessTokenLifespan: 300,
    ssoSessionIdleTimeout: 1800,
    ssoSessionMaxLifespan: 36000,
    passwordPolicy: "length(12) and maxLength(128)",
    internationalizationEnabled: true,
    supportedLocales: ["ja", "en"],
    defaultLocale: "ja",
    attributes: {frontendUrl: $frontend_url}
  }' >"$realm_file"

if "$kcadm" get "realms/$realm" --config "$config_file" >/dev/null 2>&1; then
  "$kcadm" update "realms/$realm" --config "$config_file" -f "$realm_file" >/dev/null
else
  "$kcadm" create realms --config "$config_file" -f "$realm_file" >/dev/null
fi

"$kcadm" get clients \
  --config "$config_file" \
  -r "$realm" \
  -q "clientId=$client_id" >"$clients_file"
client_count="$(jq 'length' "$clients_file")"
[[ "$client_count" == 0 || "$client_count" == 1 ]] \
  || fail "Keycloak returned duplicate clients"

client_uuid=""
if [[ "$client_count" == 1 ]]; then
  client_uuid="$(jq -er '.[0].id' "$clients_file")"
  if [[ ! -s "$client_secret_file" ]]; then
    "$kcadm" get "clients/$client_uuid/client-secret" \
      --config "$config_file" \
      -r "$realm" | jq -jer '.value' >"$client_secret_file"
  fi
elif [[ ! -s "$client_secret_file" ]]; then
  openssl rand -hex 32 >"$client_secret_file"
fi

[[ "$(wc -c <"$client_secret_file")" -ge 32 ]] \
  || fail "client secret is unexpectedly short"
chown root:root "$client_secret_file"
chmod 0600 "$client_secret_file"

jq -n \
  --arg client_id "$client_id" \
  --arg public_origin "$public_origin" \
  --arg callback_url "$callback_url" \
  --arg owner_origin "$owner_origin" \
  --arg owner_callback_url "$owner_callback_url" \
  --rawfile client_secret "$client_secret_file" \
  '{
    clientId: $client_id,
    name: "HeteroCloud Web",
    description: "HeteroCloud public console and private owner console",
    enabled: true,
    protocol: "openid-connect",
    publicClient: false,
    bearerOnly: false,
    consentRequired: false,
    standardFlowEnabled: true,
    implicitFlowEnabled: false,
    directAccessGrantsEnabled: false,
    serviceAccountsEnabled: false,
    frontchannelLogout: true,
    clientAuthenticatorType: "client-secret",
    secret: ($client_secret | rtrimstr("\n")),
    rootUrl: $public_origin,
    baseUrl: $public_origin,
    redirectUris: [$callback_url, $owner_callback_url],
    webOrigins: [$public_origin, $owner_origin],
    attributes: {
      "pkce.code.challenge.method": "S256",
      "post.logout.redirect.uris": (($public_origin + "/*") + "##" + ($owner_origin + "/*")),
      "oauth2.device.authorization.grant.enabled": "false",
      "backchannel.logout.session.required": "true",
      "backchannel.logout.revoke.offline.tokens": "false"
    }
  }' >"$client_file"

if [[ -n "$client_uuid" ]]; then
  "$kcadm" update "clients/$client_uuid" \
    --config "$config_file" \
    -r "$realm" \
    -f "$client_file" >/dev/null
else
  "$kcadm" create clients \
    --config "$config_file" \
    -r "$realm" \
    -f "$client_file" >/dev/null
fi

printf 'Keycloak realm `%s` and client `%s` reconciled.\n' "$realm" "$client_id"
printf 'Client secret retained at %s.\n' "$client_secret_file"
