#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

BIN_PATH="${TASSH_BIN:-${REPO_ROOT}/target/release/tassh}"
IMAGE_NAME="${TASSH_E2E_IMAGE:-tassh-e2e-node:latest}"
NETWORK_NAME="tassh-e2e-net"
PORT=9877
NODES=(node-a node-b node-c node-d)

log() {
  printf '[mesh-e2e] %s\n' "$*"
}

docker_exec() {
  local node="$1"
  local cmd="$2"
  docker exec "${node}" bash -lc "${cmd}"
}

status_text() {
  local node="$1"
  docker exec "${node}" /usr/local/bin/tassh status | tr -d '\r'
}

status_has() {
  local node="$1"
  local needle="$2"
  status_text "${node}" | grep -F -- "${needle}" >/dev/null
}

status_not_has() {
  local node="$1"
  local needle="$2"
  ! status_has "${node}" "${needle}"
}

wait_for() {
  local timeout_secs="$1"
  local desc="$2"
  shift 2

  local start
  start="$(date +%s)"
  while true; do
    if "$@"; then
      return 0
    fi
    if (( $(date +%s) - start >= timeout_secs )); then
      log "TIMEOUT: ${desc}"
      return 1
    fi
    sleep 1
  done
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local message="$3"
  if [[ "${expected}" != "${actual}" ]]; then
    log "ASSERT FAILED: ${message} (expected='${expected}', actual='${actual}')"
    return 1
  fi
}

count_tassh() {
  local node="$1"
  docker_exec "${node}" "pgrep -xc tassh || true"
}

count_established_inbound() {
  local node="$1"
  docker_exec "${node}" "ss -tn state established 'sport = :${PORT}' | awk 'NR>1 {c++} END {print c+0}'"
}

daemon_running() {
  local node="$1"
  [[ "$(status_text "${node}")" != "daemon not running" ]]
}

start_daemon() {
  local node="$1"
  docker_exec "${node}" "pkill -x tassh >/dev/null 2>&1 || true; rm -rf /root/.tassh; nohup env RUST_LOG=info /usr/local/bin/tassh daemon --port ${PORT} >/tmp/tassh-daemon.log 2>&1 &"
  wait_for 25 "daemon ready on ${node}" daemon_running "${node}"
}

stop_daemon() {
  local node="$1"
  docker_exec "${node}" "pkill -x tassh >/dev/null 2>&1 || true; rm -f /root/.tassh/daemon.sock"
}

start_ssh_session_from_a() {
  local target="$1"
  local label="$2"
  local pid
  pid="$(docker_exec node-a "nohup ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i /root/.ssh/id_ed25519 root@${target} 'sleep 300' >/tmp/${label}.log 2>&1 & echo \$!")"
  docker_exec node-a "/usr/local/bin/tassh notify --host ${target} --port 22 --ssh-pid ${pid}"
  printf '%s' "${pid}"
}

kill_pid_on_a() {
  local pid="$1"
  docker_exec node-a "kill ${pid} >/dev/null 2>&1 || true"
}

clipboard_hash() {
  local node="$1"
  docker_exec "${node}" ". /root/.tassh/display >/dev/null 2>&1 && timeout 3 xclip -selection clipboard -t image/png -o 2>/dev/null | sha256sum | awk '{print \$1}' || true"
}

clipboard_hash_matches() {
  local node="$1"
  local expected="$2"
  [[ "$(clipboard_hash "${node}")" == "${expected}" ]]
}

dump_diagnostics() {
  log "===== diagnostics ====="
  for node in "${NODES[@]}"; do
    log "--- ${node} status ---"
    status_text "${node}" || true
    log "--- ${node} process counts ---"
    docker_exec "${node}" "pgrep -a tassh || true; pgrep -a ssh || true" || true
    log "--- ${node} daemon log tail ---"
    docker_exec "${node}" "tail -n 80 /tmp/tassh-daemon.log 2>/dev/null || true" || true
  done
  log "======================="
}

cleanup() {
  set +e
  for node in "${NODES[@]}"; do
    docker rm -f "${node}" >/dev/null 2>&1 || true
  done
  docker network rm "${NETWORK_NAME}" >/dev/null 2>&1 || true
}

on_error() {
  set +e
  dump_diagnostics
  exit 1
}

trap on_error ERR
trap cleanup EXIT

log "checking prerequisites"
command -v docker >/dev/null

if [[ ! -x "${BIN_PATH}" ]]; then
  log "building release binary at ${BIN_PATH}"
  (cd "${REPO_ROOT}" && cargo build --release)
fi

log "building Docker test image ${IMAGE_NAME}"
docker build -f "${REPO_ROOT}/tests/docker/Dockerfile" -t "${IMAGE_NAME}" "${REPO_ROOT}" >/dev/null

cleanup

docker network create "${NETWORK_NAME}" >/dev/null
for node in "${NODES[@]}"; do
  docker run -d --name "${node}" --hostname "${node}" --network "${NETWORK_NAME}" "${IMAGE_NAME}" >/dev/null
  docker cp "${BIN_PATH}" "${node}:/usr/local/bin/tassh"
  docker_exec "${node}" "chmod +x /usr/local/bin/tassh"
done

log "configuring SSH trust from node-a to peer nodes"
docker_exec node-a "[ -f /root/.ssh/id_ed25519 ] || ssh-keygen -q -t ed25519 -N '' -f /root/.ssh/id_ed25519"
NODE_A_PUBKEY="$(docker exec node-a cat /root/.ssh/id_ed25519.pub)"
for peer in node-b node-c node-d; do
  printf '%s\n' "${NODE_A_PUBKEY}" | docker exec -i "${peer}" bash -lc "cat > /root/.ssh/authorized_keys && chmod 700 /root/.ssh && chmod 600 /root/.ssh/authorized_keys"
done

log "starting daemons on node-a and node-b"
start_daemon node-a
start_daemon node-b

log "test 1: first session connects, second session reuses the same daemon connection"
PID_B1="$(start_ssh_session_from_a node-b ssh-b-1)"
wait_for 25 "node-a syncing to node-b after first session" status_has node-a "node-b -- syncing (1 SSH session)"
assert_eq "1" "$(count_tassh node-a)" "node-a should have exactly one daemon process"
assert_eq "1" "$(count_tassh node-b)" "node-b should have exactly one daemon process"
ESTABLISHED_BEFORE="$(count_established_inbound node-b)"
assert_eq "1" "${ESTABLISHED_BEFORE}" "node-b should have one established tassh inbound connection"

PID_B2="$(start_ssh_session_from_a node-b ssh-b-2)"
wait_for 25 "node-a shows two sessions to node-b" status_has node-a "node-b -- syncing (2 SSH sessions)"
ESTABLISHED_AFTER="$(count_established_inbound node-b)"
assert_eq "${ESTABLISHED_BEFORE}" "${ESTABLISHED_AFTER}" "second SSH session must not create a second daemon TCP connection"

kill_pid_on_a "${PID_B1}"
wait_for 25 "node-a remains synced after first session closes" status_has node-a "node-b -- syncing (1 SSH session)"

kill_pid_on_a "${PID_B2}"
wait_for 25 "node-b peer removed after all sessions end" status_not_has node-a "node-b --"

log "test 2: node-a syncs to multiple peers and clipboard fan-out reaches all"
start_daemon node-c
PID_B3="$(start_ssh_session_from_a node-b ssh-b-3)"
PID_C1="$(start_ssh_session_from_a node-c ssh-c-1)"
wait_for 30 "node-a syncing to node-b" status_has node-a "node-b -- syncing (1 SSH session)"
wait_for 30 "node-a syncing to node-c" status_has node-a "node-c -- syncing (1 SSH session)"

CLIP_PNG="${REPO_ROOT}/target/tassh-e2e-clip.png"
printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO2f7iwAAAAASUVORK5CYII=' | base64 -d > "${CLIP_PNG}"
EXPECTED_HASH="$(sha256sum "${CLIP_PNG}" | awk '{print $1}')"
docker cp "${CLIP_PNG}" node-a:/tmp/clip.png

docker_exec node-a "/usr/local/bin/tassh inject --png-file /tmp/clip.png"

wait_for 30 "node-b clipboard hash matches source" clipboard_hash_matches node-b "${EXPECTED_HASH}"
wait_for 30 "node-c clipboard hash matches source" clipboard_hash_matches node-c "${EXPECTED_HASH}"

kill_pid_on_a "${PID_B3}"
kill_pid_on_a "${PID_C1}"
wait_for 25 "node-b peer removed after cleanup" status_not_has node-a "node-b --"
wait_for 25 "node-c peer removed after cleanup" status_not_has node-a "node-c --"

log "test 3: session starts before remote daemon; connection comes up after remote daemon starts"
stop_daemon node-d
PID_D1="$(start_ssh_session_from_a node-d ssh-d-1)"
wait_for 25 "node-a tracks node-d session before daemon exists" status_has node-a "node-d --"

start_daemon node-d
wait_for 30 "node-a eventually syncs with node-d after daemon start" status_has node-a "node-d -- syncing (1 SSH session)"

kill_pid_on_a "${PID_D1}"
wait_for 25 "node-d peer removed after final session closes" status_not_has node-a "node-d --"

log "all daemon mesh e2e assertions passed"
