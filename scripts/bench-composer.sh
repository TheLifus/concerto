#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONCERTO="$ROOT/target/debug/concerto"
WORKDIR="$(mktemp -d)"

trap 'rm -rf "$WORKDIR"' EXIT

now_ms() {
    perl -MTime::HiRes=time -e 'printf "%.0f", time * 1000'
}

timed() {
    local start
    local end

    start="$(now_ms)"
    "$@" >/dev/null
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
        composer:2 \
        install \
        --ignore-platform-reqs \
        --no-interaction \
        --no-progress \
        --quiet \
        --no-scripts
}

concerto_install() {
    local project="$1"

    (
        cd "$project"
        CONCERTO_DEBUG_PERF=1 "$CONCERTO" install
    )
}

package_count() {
    local project="$1"

    find "$project/vendor" -mindepth 2 -maxdepth 2 \( -type d -o -type l \) | wc -l | tr -d ' '
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
    local packages
    local lock_divisor
    local lock_speedup

    composer_cold_ms="$(timed composer_install "$composer_project")"
    composer_warm_ms="$(timed composer_install "$composer_project")"
    concerto_cold_ms="$(timed concerto_install "$concerto_project")"
    concerto_lock_ms="$(timed concerto_install "$concerto_project")"

    rm -rf "$concerto_project/vendor"
    concerto_relink_ms="$(timed concerto_install "$concerto_project")"
    packages="$(package_count "$concerto_project")"
    lock_divisor="$concerto_lock_ms"

    if [ "$lock_divisor" -lt 1 ]; then
        lock_divisor=1
    fi

    lock_speedup=$((composer_warm_ms / lock_divisor))

    printf '%-11s %8s %13s %13s %13s %13s %15s %9sx\n' \
        "$name" \
        "$packages" \
        "$composer_cold_ms" \
        "$composer_warm_ms" \
        "$concerto_cold_ms" \
        "$concerto_lock_ms" \
        "$concerto_relink_ms" \
        "$lock_speedup"
}

command -v docker >/dev/null || {
    echo "Docker is required to benchmark Composer." >&2
    exit 1
}

cargo build --quiet --manifest-path "$ROOT/Cargo.toml"
docker image inspect composer:2 >/dev/null 2>&1 || docker pull composer:2 >/dev/null 2>&1

echo "Composer runs with --ignore-platform-reqs because Concerto does not enforce platform yet."
printf '%-11s %8s %13s %13s %13s %13s %15s %10s\n' \
    "case" \
    "packages" \
    "composer_cold" \
    "composer_warm" \
    "concerto_cold" \
    "concerto_lock" \
    "concerto_relink" \
    "warm_gain"

bench_case "direct" '{"require":{"psr/log":"^3.0"}}'
bench_case "transitive" '{"require":{"monolog/monolog":"^3.0"}}'
bench_case "multi" '{
  "require": {
    "monolog/monolog": "^3.0",
    "symfony/console": "^8.0",
    "guzzlehttp/guzzle": "^7.0",
    "ramsey/uuid": "^4.0",
    "league/flysystem": "^3.0"
  }
}'
