#!/usr/bin/env bash
# Copyright © 2026 Jalapeno Labs
#
# Cuts the next release: tags the newest commit on main whose checks all passed, and
# starts publish.yml on the tag. Run by .github/workflows/release.yml, from a checkout of
# main with its whole history. See docs/ci.md.
#
# Nothing is cut when nothing but documentation changed since the last release, or when no
# commit since it has passed every check yet.
#
# Needs GH_TOKEN (contents and actions write, checks read), GITHUB_REPOSITORY, and BUMP
# (patch, minor, or major).

set -euo pipefail

: "${GH_TOKEN:?GH_TOKEN is not set}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"
: "${BUMP:?BUMP is not set}"

# How far back a release may reach for a green commit. A day of main rarely holds more.
readonly MAX_CANDIDATES=50
# The job that runs this. Its own check run sits on main's newest commit while it runs, and
# must not count as that commit's checks still running.
readonly OWN_CHECK_NAME="Cut release"

say() {
  echo "$*"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "$*" >> "${GITHUB_STEP_SUMMARY}"
  fi
}

# `green`, `pending`, or `failed`, from every check run on the commit but this job's own.
# A commit with no check run at all is `pending`: its workflows have not started.
checks_state() {
  local runs
  runs="$(gh api "repos/${GITHUB_REPOSITORY}/commits/$1/check-runs?per_page=100" \
    --jq ".check_runs[] | select(.name != \"${OWN_CHECK_NAME}\") | .status + \" \" + (.conclusion // \"\")")"
  if [ -z "${runs}" ]; then
    echo pending
    return
  fi
  local status conclusion state=green
  while read -r status conclusion; do
    if [ "${status}" != "completed" ]; then
      state=pending
      continue
    fi
    case "${conclusion}" in
      success | skipped | neutral) ;;
      *)
        echo failed
        return
        ;;
    esac
  done <<< "${runs}"
  echo "${state}"
}

# The version after $1 (X.Y.Z), raising the part BUMP names.
next_version() {
  local major minor patch
  IFS=. read -r major minor patch <<< "$1"
  case "${BUMP}" in
    major) echo "$((major + 1)).0.0" ;;
    minor) echo "${major}.$((minor + 1)).0" ;;
    patch) echo "${major}.${minor}.$((patch + 1))" ;;
    *)
      echo "BUMP must be patch, minor, or major, not ${BUMP}" >&2
      return 1
      ;;
  esac
}

last_tag="$(git tag --list 'v*' | grep -Ex 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -V | tail -n 1 || true)"
range="HEAD"
if [ -n "${last_tag}" ]; then
  range="${last_tag}..HEAD"
fi

candidate=""
while read -r commit; do
  [ -n "${commit}" ] || continue
  state="$(checks_state "${commit}")"
  echo "${commit::12}: checks ${state}"
  if [ "${state}" = "green" ]; then
    candidate="${commit}"
    break
  fi
done <<< "$(git rev-list --first-parent --max-count="${MAX_CANDIDATES}" "${range}")"

if [ -z "${candidate}" ]; then
  say "No release: no commit on main since ${last_tag:-the start} has passed every check."
  exit 0
fi

if [ -n "${last_tag}" ] \
  && git diff --quiet "${last_tag}" "${candidate}" -- . ':(exclude)docs/**' ':(exclude)*.md'; then
  say "No release: only documentation changed since ${last_tag}."
  exit 0
fi

if [ -z "${last_tag}" ]; then
  version="0.1.0"
else
  version="$(next_version "${last_tag#v}")"
fi

gh api "repos/${GITHUB_REPOSITORY}/git/refs" \
  --method POST \
  --field ref="refs/tags/v${version}" \
  --field sha="${candidate}" > /dev/null
gh workflow run publish.yml --repo "${GITHUB_REPOSITORY}" --ref "v${version}"

say "Released v${version} at ${candidate::12}. publish.yml is building its images."
