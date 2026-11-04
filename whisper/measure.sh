#!/usr/bin/env bash
# Copyright © 2026 Jalapeno Labs
#
# Measures how long Whisper takes per hour of audio on this host, run the way the engine runs
# it (api/src/whisper/): one container with no network, its CPUs capped at the threads it
# decodes on, yielding to every other container under contention. See docs/meetings.md,
# Transcription, where the results are recorded.
#
#   whisper/measure.sh [--image IMAGE] [--threads THREADS] [--output TRANSCRIPT] [RECORDING]
#
# IMAGE is elysium-whisper:local unless given, the tag compose.build.yml builds. THREADS is
# what the engine would use on this host unless given: half its CPUs, at most 16. RECORDING is
# LibriVox's public-domain reading of Federalist No. 10 (18:15) unless given, downloaded and
# checked against its SHA-256. It runs under nice, so the host's own services keep priority,
# and prints the host, the timing, and the peak memory, for the table in the docs. --output
# keeps the transcript, to read what came back.
set -euo pipefail

image=elysium-whisper:local
threads=""
output=""
recording=""
while (($#)); do
  case "$1" in
    --image) image="$2"; shift 2 ;;
    --threads) threads="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    -h | --help) sed -n '3,16p' "$0"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 64 ;;
    *) recording="$1"; shift ;;
  esac
done

# The engine's rule (api/src/whisper/mod.rs, transcription_threads): half the CPUs, which on a
# host with two threads per core is about one per physical core, and no more than 16, past
# which CTranslate2 gains little.
if [[ -z "$threads" ]]; then
  threads=$(($(nproc) / 2))
  ((threads < 1)) && threads=1
  ((threads > 16)) && threads=16
fi

scratch="$(mktemp -d)"
container="elysium-whisper-measure-$$"
cleanup() {
  docker rm --force "$container" > /dev/null 2>&1 || true
  rm -rf "$scratch"
}
trap cleanup EXIT

if [[ -z "$recording" ]]; then
  recording="$scratch/federalist-10.mp3"
  curl --fail --silent --show-error --location --output "$recording" \
    https://archive.org/download/federalist_papers_librivox/federalist-papers_10_64kb.mp3
  echo "e38bb78e70ca4cacb168ea6fcb49863b1fe42fc24aa00905c2e8e26dabf78312  $recording" \
    | sha256sum --check --strict --quiet
fi

# The same limits as the engine's container (api/src/whisper/container.rs).
started="$(date +%s.%N)"
nice -n 10 docker run --interactive --name "$container" \
  --network none --cpus "$threads" --cpu-shares 256 --memory 12g --memory-swap 12g --pids-limit 256 \
  --cap-drop ALL --security-opt no-new-privileges \
  "$image" --threads "$threads" < "$recording" > "$scratch/transcript.json" &
run=$!

# Docker reports memory in mixed units (MiB, GiB); the peak is kept in MiB.
peak_mib=0
while kill -0 "$run" 2> /dev/null; do
  usage="$(docker stats --no-stream --format '{{.MemUsage}}' "$container" 2> /dev/null || true)"
  mib="$(awk '{
    value = $1 + 0
    if ($1 ~ /GiB/) value *= 1024
    else if ($1 ~ /KiB/) value /= 1024
    else if ($1 ~ /B$/ && $1 !~ /iB/) value /= 1048576
    printf "%d", value
  }' <<< "${usage:-0MiB}")"
  ((mib > peak_mib)) && peak_mib="$mib"
  sleep 2
done
wait "$run"
finished="$(date +%s.%N)"
[[ -n "$output" ]] && cp "$scratch/transcript.json" "$output"

audio_ms="$(grep -o '"durationMs":[0-9]*' "$scratch/transcript.json" | cut -d: -f2)"
awk -v started="$started" -v finished="$finished" -v audio_ms="$audio_ms" \
  -v threads="$threads" -v peak_mib="$peak_mib" \
  -v cpu="$(lscpu | sed -n 's/^Model name: *//p' | head -n 1)" \
  -v cpus="$(nproc)" -v kernel="$(uname -r)" \
  -v memory="$(free -g | awk '/^Mem:/ { print $2 }')" '
  BEGIN {
    wall = finished - started
    audio = audio_ms / 1000
    printf "Host:            %s, %d CPUs, %d GiB, kernel %s\n", cpu, cpus, memory, kernel
    printf "Threads:         %d (--cpus %d)\n", threads, threads
    printf "Audio:           %.1f s\n", audio
    printf "Wall time:       %.1f s\n", wall
    printf "Per hour:        %.1f minutes per hour of audio\n", wall / audio * 60
    printf "Real-time:       %.2fx (audio seconds per wall second)\n", audio / wall
    printf "Peak memory:     %d MiB\n", peak_mib
  }'
