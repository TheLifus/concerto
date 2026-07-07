#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="$(mktemp -d)"
COMPOSER_IMAGE="${COMPOSER_IMAGE:-composer:2}"
CONCERTO_IMAGE="${CONCERTO_IMAGE:-concerto-bench:local}"
RUST_IMAGE="${RUST_IMAGE:-rust:1-alpine}"

CASE_COUNT=0
TOTAL_PACKAGES=0
TOTAL_COMPOSER_COLD_MS=0
TOTAL_COMPOSER_WARM_MS=0
TOTAL_CONCERTO_COLD_MS=0
TOTAL_CONCERTO_LOCK_MS=0
TOTAL_CONCERTO_RELINK_MS=0
TOTAL_CONCERTO_TRUST_RELINK_MS=0

trap 'rm -rf "$WORKDIR"' EXIT

now_ms() {
    perl -MTime::HiRes=time -e 'printf "%.0f", time * 1000'
}

timed() {
    local start
    local end
    local status

    start="$(now_ms)"
    set +e
    "$@" >/dev/null
    status="$?"
    set -e

    if [ "$status" -ne 0 ]; then
        return "$status"
    fi

    end="$(now_ms)"

    echo $((end - start))
}

write_composer_json() {
    local project="$1"
    local json="$2"

    mkdir -p "$project"
    printf '%s\n' "$json" >"$project/composer.json"
}

composer_install() {
    local project="$1"

    docker run --rm \
        --user "$(id -u):$(id -g)" \
        --volume "$project:/app" \
        --workdir /app \
        --env COMPOSER_HOME=/tmp/composer-home \
        "$COMPOSER_IMAGE" \
        install \
        --ignore-platform-reqs \
        --no-interaction \
        --no-progress \
        --quiet \
        --no-scripts
}

concerto_install() {
    local project="$1"
    shift

    docker run --rm \
        --user "$(id -u):$(id -g)" \
        --volume "$project:/app" \
        --workdir /app \
        --env CONCERTO_DEBUG_PERF=1 \
        "$CONCERTO_IMAGE" \
        install \
        "$@"
}

package_count() {
    local project="$1"

    find "$project/vendor" -mindepth 2 -maxdepth 2 \( -type d -o -type l \) | wc -l | tr -d ' '
}

compare_time() {
    local baseline_ms="$1"
    local candidate_ms="$2"

    awk -v baseline="$baseline_ms" -v candidate="$candidate_ms" '
        BEGIN {
            if (baseline < 1) {
                baseline = 1
            }

            if (candidate < 1) {
                candidate = 1
            }

            if (candidate <= baseline) {
                printf "%.1fx faster", baseline / candidate
            } else {
                printf "%.1fx slower", candidate / baseline
            }
        }
    '
}

track_case() {
    local packages="$1"
    local composer_cold_ms="$2"
    local composer_warm_ms="$3"
    local concerto_cold_ms="$4"
    local concerto_lock_ms="$5"
    local concerto_relink_ms="$6"
    local concerto_unsafe_relink_ms="$7"

    CASE_COUNT=$((CASE_COUNT + 1))
    TOTAL_PACKAGES=$((TOTAL_PACKAGES + packages))
    TOTAL_COMPOSER_COLD_MS=$((TOTAL_COMPOSER_COLD_MS + composer_cold_ms))
    TOTAL_COMPOSER_WARM_MS=$((TOTAL_COMPOSER_WARM_MS + composer_warm_ms))
    TOTAL_CONCERTO_COLD_MS=$((TOTAL_CONCERTO_COLD_MS + concerto_cold_ms))
    TOTAL_CONCERTO_LOCK_MS=$((TOTAL_CONCERTO_LOCK_MS + concerto_lock_ms))
    TOTAL_CONCERTO_RELINK_MS=$((TOTAL_CONCERTO_RELINK_MS + concerto_relink_ms))
    TOTAL_CONCERTO_TRUST_RELINK_MS=$((TOTAL_CONCERTO_TRUST_RELINK_MS + concerto_unsafe_relink_ms))
}

bench_case() {
    local name="$1"
    local json="$2"
    local composer_project="$WORKDIR/$name/composer"
    local concerto_project="$WORKDIR/$name/concerto"

    write_composer_json "$composer_project" "$json"
    write_composer_json "$concerto_project" "$json"

    local composer_cold_ms
    local composer_warm_ms
    local concerto_cold_ms
    local concerto_lock_ms
    local concerto_relink_ms
    local concerto_unsafe_relink_ms
    local packages
    local cold_result
    local lock_result

    composer_cold_ms="$(timed composer_install "$composer_project")"
    composer_warm_ms="$(timed composer_install "$composer_project")"
    concerto_cold_ms="$(timed concerto_install "$concerto_project")"
    concerto_lock_ms="$(timed concerto_install "$concerto_project")"

    rm -rf "$concerto_project/vendor"
    concerto_relink_ms="$(timed concerto_install "$concerto_project")"
    rm -rf "$concerto_project/vendor"
    concerto_unsafe_relink_ms="$(timed concerto_install "$concerto_project" --unsafe-trust-store)"
    packages="$(package_count "$concerto_project")"
    cold_result="$(compare_time "$composer_cold_ms" "$concerto_cold_ms")"
    lock_result="$(compare_time "$composer_warm_ms" "$concerto_lock_ms")"

    track_case \
        "$packages" \
        "$composer_cold_ms" \
        "$composer_warm_ms" \
        "$concerto_cold_ms" \
        "$concerto_lock_ms" \
        "$concerto_relink_ms" \
        "$concerto_unsafe_relink_ms"

    printf '%-16s %8s %13s %13s %13s %13s %15s %15s %-14s %-14s\n' \
        "$name" \
        "$packages" \
        "$composer_cold_ms" \
        "$composer_warm_ms" \
        "$concerto_cold_ms" \
        "$concerto_lock_ms" \
        "$concerto_relink_ms" \
        "$concerto_unsafe_relink_ms" \
        "$cold_result" \
        "$lock_result"
}

average() {
    local total="$1"

    echo $((total / CASE_COUNT))
}

print_summary() {
    local avg_packages
    local avg_composer_cold
    local avg_composer_warm
    local avg_concerto_cold
    local avg_concerto_lock
    local avg_concerto_relink
    local avg_concerto_unsafe_relink

    avg_packages="$(average "$TOTAL_PACKAGES")"
    avg_composer_cold="$(average "$TOTAL_COMPOSER_COLD_MS")"
    avg_composer_warm="$(average "$TOTAL_COMPOSER_WARM_MS")"
    avg_concerto_cold="$(average "$TOTAL_CONCERTO_COLD_MS")"
    avg_concerto_lock="$(average "$TOTAL_CONCERTO_LOCK_MS")"
    avg_concerto_relink="$(average "$TOTAL_CONCERTO_RELINK_MS")"
    avg_concerto_unsafe_relink="$(average "$TOTAL_CONCERTO_TRUST_RELINK_MS")"

    echo
    echo "Average over $CASE_COUNT cases ($avg_packages packages average):"
    printf '  Cold install: Concerto is %s than Composer (%sms vs %sms).\n' \
        "$(compare_time "$avg_composer_cold" "$avg_concerto_cold")" \
        "$avg_concerto_cold" \
        "$avg_composer_cold"
    printf '  Lock install: Concerto is %s than Composer warm (%sms vs %sms).\n' \
        "$(compare_time "$avg_composer_warm" "$avg_concerto_lock")" \
        "$avg_concerto_lock" \
        "$avg_composer_warm"
    printf '  Vendor relink: Concerto averages %sms.\n' "$avg_concerto_relink"
    printf '  Unsafe trusted relink: Concerto averages %sms.\n' "$avg_concerto_unsafe_relink"
}

command -v docker >/dev/null || {
    echo "Docker is required to benchmark Composer." >&2
    exit 1
}

docker image inspect "$COMPOSER_IMAGE" >/dev/null 2>&1 || docker pull "$COMPOSER_IMAGE" >/dev/null 2>&1
docker build \
    --quiet \
    --file "$ROOT/scripts/bench-concerto.Dockerfile" \
    --build-arg COMPOSER_IMAGE="$COMPOSER_IMAGE" \
    --build-arg RUST_IMAGE="$RUST_IMAGE" \
    --tag "$CONCERTO_IMAGE" \
    "$ROOT" >/dev/null

echo "Composer and Concerto run in Docker with $COMPOSER_IMAGE."
echo "Composer runs with --ignore-platform-reqs."
printf '%-16s %8s %13s %13s %13s %13s %15s %15s %-14s %-14s\n' \
    "case" \
    "packages" \
    "composer_cold" \
    "composer_warm" \
    "concerto_cold" \
    "concerto_lock" \
    "concerto_relink" \
    "concerto_unsafe" \
    "cold_result" \
    "lock_result"

bench_case "direct" '{"require":{"psr/log":"^3.0"}}'
bench_case "transitive" '{"require":{"monolog/monolog":"^3.0"}}'
bench_case "multi-app" '{
  "require": {
    "monolog/monolog": "^3.0",
    "brick/math": "^0.14",
    "guzzlehttp/guzzle": "^7.0",
    "ramsey/uuid": "^4.0",
    "league/flysystem": "^3.0"
  }
}'
bench_case "multi-symfony" '{
  "require": {
    "symfony/console": "^7.0",
    "symfony/filesystem": "^7.0",
    "symfony/finder": "^7.0",
    "symfony/process": "^7.0",
    "symfony/yaml": "^7.0"
  }
}'
bench_case "multi-utils" '{
  "require": {
    "brick/math": "^0.14",
    "nesbot/carbon": "^3.0",
    "ramsey/uuid": "^4.0",
    "symfony/string": "^7.0",
    "vlucas/phpdotenv": "^5.6"
  }
}'
bench_case "multi-http" '{
  "require": {
    "guzzlehttp/guzzle": "^7.0",
    "nyholm/psr7": "^1.8",
    "psr/http-client": "^1.0",
    "psr/http-factory": "^1.0",
    "symfony/http-client": "^7.0"
  }
}'

print_summary
