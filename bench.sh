#!/usr/bin/env bash
# Host setup/restore and the standard run matrix. `setup` and `restore` need sudo.
#   ./bench.sh setup            performance governor, no turbo, SMT sibling of CPU 4 offline
#   ./bench.sh run  <dir> [hashes]   16 runs: bits {16,24,32,64} x seed {0,1} x hash (default: blake3-xxh xxh3)
#   ./bench.sh large <dir>      20M-key table (no SQLite) so the table exceeds L2/L3
#   ./bench.sh fprate <dir>     false-positive rates at 871,000 keys in a 900,000-key table (no sudo or pinning needed)
#   ./bench.sh restore          put the saved governor/turbo/CPU state back
set -euo pipefail
cd "$(dirname "$0")"
CPU=4
SIBLING=5
STATE=.host-state
BIN=target/release/cuckoo-bench

case "${1:-}" in
setup)
  [ -e "$STATE" ] && { echo "already set up ($STATE exists); run restore first" >&2; exit 1; }
  {
    echo "governor=$(cat /sys/devices/system/cpu/cpu$CPU/cpufreq/scaling_governor)"
    echo "no_turbo=$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)"
    echo "sibling=$(cat /sys/devices/system/cpu/cpu$SIBLING/online)"
  } > "$STATE"
  sudo cpupower -c all frequency-set -g performance >/dev/null
  echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo >/dev/null
  echo 0 | sudo tee /sys/devices/system/cpu/cpu$SIBLING/online >/dev/null
  cat "$STATE"; echo "host locked; pinned CPU is $CPU"
  ;;
restore)
  . "$STATE"
  sudo cpupower -c all frequency-set -g "$governor" >/dev/null
  echo "$no_turbo" | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo >/dev/null
  echo "$sibling" | sudo tee /sys/devices/system/cpu/cpu$SIBLING/online >/dev/null
  rm -f "$STATE"; echo restored
  ;;
run)
  out=${2:?output dir}
  cargo build --release
  for hash in ${3:-blake3-xxh xxh3}; do for bits in 16 24 32 64; do for seed in 0 1; do
    taskset -c $CPU $BIN --count 900000 --queries 100000 --repeats 15 --seed $seed \
      --fingerprint-bits $bits --hash $hash --output "$out/$hash-fp$bits-seed$seed"
  done; done; done
  ;;
large)
  out=${2:?output dir}
  cargo build --release
  for bits in 16 32; do
    taskset -c $CPU $BIN --count 20000000 --queries 1000000 --repeats 15 --seed 0 --no-sqlite \
      --fingerprint-bits $bits --output "$out/large-fp$bits-seed0"
  done
  ;;
fprate)
  out=${2:?output dir}
  cargo build --release
  mkdir -p "$out"
  for hash in blake3-xxh xxh3; do
    target/release/fprate --bits 16 --hash $hash --probes 1000000000  --output "$out/$hash-fp16.json"
    target/release/fprate --bits 24 --hash $hash --probes 3000000000  --output "$out/$hash-fp24.json"
    target/release/fprate --bits 32 --hash $hash --probes 30000000000 --output "$out/$hash-fp32.json"
  done
  ;;
*) sed -n '2,8p' "$0"; exit 1 ;;
esac
