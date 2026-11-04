#!/usr/bin/env bash
# Copyright © 2026 Jalapeno Labs
#
# Measures how many passages a second the embedding model embeds on this host, run the way
# compose runs it: no route out, its CPUs capped, yielding to every other container under
# contention. See docs/knowledge.md, Embeddings, where the results are recorded.
#
#   embeddings/measure.sh [--image IMAGE] [--cpus CPUS] [--passages COUNT]
#
# IMAGE is elysium-embeddings:local unless given, the tag compose.build.yml builds. CPUS is
# compose's cap, 4, unless given. The passages are this checkout's docs/*.md, cut as the
# knowledge base cuts text (paragraphs packed up to 1,600 characters), the first COUNT of them
# (all, unless given), and sent the way the engine sends them: 32 a request, each with the
# model's document prefix. It runs under nice, so the host's own services keep priority, and
# prints the host, the rate, and the peak memory, for the table in the docs. Run it from the
# repository root.
set -euo pipefail

image=elysium-embeddings:local
cpus=4
count=0
while (($#)); do
  case "$1" in
    --image) image="$2"; shift 2 ;;
    --cpus) cpus="$2"; shift 2 ;;
    --passages) count="$2"; shift 2 ;;
    -h | --help) sed -n '3,16p' "$0"; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 64 ;;
  esac
done

container="elysium-embeddings-measure-$$"
cleanup() { docker rm --force "$container" > /dev/null 2>&1 || true; }
trap cleanup EXIT

# The same limits as compose's `embeddings` service. No network at all here: the requests
# reach it through `docker exec`, which needs none.
nice -n 10 docker run --detach --name "$container" \
  --network none --cpus "$cpus" --cpu-shares 256 --memory 3g --memory-swap 3g --pids-limit 256 \
  --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
  "$image" > /dev/null
until docker exec "$container" curl -sf http://127.0.0.1:8080/health > /dev/null 2>&1; do
  if [[ "$(docker inspect --format '{{.State.Running}}' "$container")" != true ]]; then
    docker logs "$container" >&2
    exit 1
  fi
  sleep 1
done

# Docker reports memory in mixed units (MiB, GiB); the peak is kept in MiB, in a file the
# sampler rewrites, since it runs beside the requests.
peak_file="$(mktemp)"
echo 0 > "$peak_file"
sample() {
  local peak=0 mib usage
  while docker inspect "$container" > /dev/null 2>&1; do
    usage="$(docker stats --no-stream --format '{{.MemUsage}}' "$container" 2> /dev/null || true)"
    mib="$(awk '{
      value = $1 + 0
      if ($1 ~ /GiB/) value *= 1024
      else if ($1 ~ /KiB/) value /= 1024
      printf "%d", value
    }' <<< "${usage:-0MiB}")"
    ((mib > peak)) && peak="$mib" && echo "$peak" > "$peak_file"
    sleep 2
  done
}
sample &
sampler=$!

# Isolated (-I), so nothing in the working directory changes what Python imports.
python3 -I - "$container" "$count" "$cpus" "$peak_file" <<'EOF'
import json
import os
import pathlib
import platform
import subprocess
import sys
import time

container, count, cpus, peak_file = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]

# Much as knowledge::passages::split cuts text: paragraphs packed back together up to 1,600
# characters. A longer paragraph is simply cut short here, which changes no passage's size.
LIMIT = 1600
passages = []
for path in sorted(pathlib.Path("docs").glob("*.md")):
    current = ""
    for paragraph in path.read_text().split("\n\n"):
        paragraph = paragraph.strip()[:LIMIT]
        if not paragraph:
            continue
        if current and len(current) + 2 + len(paragraph) > LIMIT:
            passages.append(current)
            current = ""
        current = f"{current}\n\n{paragraph}" if current else paragraph
    if current:
        passages.append(current)
if count:
    passages = passages[:count]


def embed(batch):
    body = json.dumps({"inputs": [f"search_document: {passage}" for passage in batch]})
    answer = subprocess.run(
        ["docker", "exec", "-i", container, "curl", "-sS", "--fail-with-body",
         "-H", "content-type: application/json", "--data-binary", "@-",
         "http://127.0.0.1:8080/embed"],
        input=body.encode(), capture_output=True, check=True,
    )
    vectors = json.loads(answer.stdout)
    assert len(vectors) == len(batch) and all(len(vector) == 768 for vector in vectors)


started = time.monotonic()
for index in range(0, len(passages), 32):
    embed(passages[index:index + 32])
elapsed = time.monotonic() - started

cpu = next(
    (line.split(":", 1)[1].strip() for line in open("/proc/cpuinfo") if line.startswith("model name")),
    platform.processor(),
)
characters = sum(len(passage) for passage in passages) / len(passages)
memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") >> 30
print(f"Host:            {cpu}, {os.cpu_count()} CPUs, {memory} GiB, kernel {platform.release()}")
print(f"CPUs:            {cpus} (--cpus {cpus})")
print(f"Passages:        {len(passages)}, {characters:.0f} characters on average")
print(f"Wall time:       {elapsed:.1f} s")
print(f"Rate:            {len(passages) / elapsed:.1f} passages a second, "
      f"{len(passages) / elapsed * 3600:,.0f} an hour")
print(f"Peak memory:     {open(peak_file).read().strip()} MiB")
EOF

cleanup
wait "$sampler" 2> /dev/null || true
rm -f "$peak_file"
