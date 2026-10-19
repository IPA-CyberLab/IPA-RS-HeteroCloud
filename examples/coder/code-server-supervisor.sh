#!/bin/sh
# Keep IDE recovery independent of the persistent home filesystem.
set -u
umask 077
runtime_dir="${CODE_SERVER_RUNTIME_DIR:-/tmp/coder-code-server-$(id -u)}"
code_server="${CODE_SERVER_BIN:-/opt/code-server/bin/code-server}"
health_url="${CODE_SERVER_HEALTH_URL:-http://127.0.0.1:13337/healthz}"
mkdir -p "$runtime_dir" || exit 1
exec 9>"$runtime_dir/supervisor.lock" || exit 1
flock -n 9 || exit 0

workspace_dir="$HOME/projects"
if [ ! -d "$workspace_dir" ]; then
  mkdir -p "$workspace_dir" 2>/dev/null || workspace_dir="$HOME"
fi

child=
stop() {
  if [ -n "$child" ]; then
    kill -TERM "$child" 2>/dev/null || true
    wait "$child" 2>/dev/null || true
  fi
  exit 0
}
trap stop TERM INT

while :; do
  # An existing healthy IDE can be adopted during supervisor upgrades without
  # disconnecting its terminals. Start a child only after that listener exits.
  if curl -fsS --max-time 2 "$health_url" >/dev/null 2>&1; then
    sleep 2
    continue
  fi
  # Rotate between child launches; diagnostic failures must never stop retries.
  for log in code-server supervisor; do
    size=$(stat -c %s "$runtime_dir/$log.log" 2>/dev/null) || size=0
    if [ "$size" -gt 2097152 ]; then
      mv -f "$runtime_dir/$log.log" "$runtime_dir/$log.log.previous" 2>/dev/null || true
    fi
  done
  "$code_server" --auth none --bind-addr 127.0.0.1:13337 --disable-telemetry "$workspace_dir" >>"$runtime_dir/code-server.log" 2>&1 &
  child=$!
  status=0
  wait "$child" || status=$?
  child=
  printf 'code-server exited (status=%s); restarting\n' "$status" >>"$runtime_dir/supervisor.log" 2>/dev/null || true
  sleep 2
done
