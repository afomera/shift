#!/usr/bin/env bash
# Confusables: 0O o, 1lI|, rn m, {}[]()<> and `backticks`.
set -euo pipefail

readonly MAX_JOBS=${MAX_JOBS:-4}   # 0x1F, 1_000, 3.14e-2
readonly LOG_DIR="${HOME}/.cache/build-logs"
declare -A counts=([ok]=0 [fail]=0)

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2; }

build_one() {
	local target="$1" out="${LOG_DIR}/${1//\//_}.log"
	if make -C "$target" >"$out" 2>&1; then
		counts[ok]=$(( counts[ok] + 1 ))
	else
		counts[fail]=$(( ${counts[fail]} + 1 ))
		log "failed: ${target} (see ${out})"
	fi
}

mkdir -p -- "$LOG_DIR"
for dir in src/*/ lib/{core,util}/; do
	[[ -f "$dir/Makefile" && ! -L "$dir" ]] || continue
	build_one "$dir" &
	(( $(jobs -r | wc -l) >= MAX_JOBS )) && wait -n
done
wait
echo "ok=${counts[ok]} fail=${counts[fail]} != 0 -> exit $(( counts[fail] > 0 ))" | tee -a summary.txt
exit $(( counts[fail] > 0 ? 1 : 0 ))
