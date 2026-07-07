#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="$(mktemp -d)"
ITERATIONS="${ITERATIONS:-15}"
CONCERTO_BIN="${CONCERTO_BIN:-$ROOT/target/release/concerto}"

trap 'rm -rf "$WORKDIR"' EXIT

now_ms() {
    perl -MTime::HiRes=time -e 'printf "%.0f", time * 1000'
}

event_ms() {
    local log="$1"
    local event="$2"

    awk -v event="$event" '
        $1 == event {
            value = $2
            sub(/ms$/, "", value)
            sum += value
        }
        END { print sum + 0 }
    ' "$log"
}

stat_col() {
    local file="$1"
    local column="$2"

    COLUMN="$column" perl -ane '
        push @values, $F[$ENV{COLUMN} - 1];
        END {
            @values = sort { $a <=> $b } @values;
            $count = scalar @values;
            $median = $values[int(($count - 1) / 2)];
            $p95_index = int($count * 0.95 + 0.999999) - 1;
            $p95 = $values[$p95_index];
            printf "median=%sms p95=%sms min=%sms max=%sms", $median, $p95, $values[0], $values[-1];
        }
    ' "$file"
}

record_run() {
    local project="$1"
    local output="$2"
    shift 2
    local log="$project/.concerto/logs/perf.log"
    local start
    local end

    rm -f "$log"
    rm -rf "$project/vendor"
    start="$(now_ms)"
    (cd "$project" && CONCERTO_DEBUG_PERF=1 "$CONCERTO_BIN" install "$@" >/dev/null)
    end="$(now_ms)"

    printf '%s %s %s %s %s %s %s %s %s\n' \
        "$((end - start))" \
        "$(event_ms "$log" platform_current)" \
        "$(event_ms "$log" lockfile_sources_prepare)" \
        "$(event_ms "$log" archive_hash_reuse)" \
        "$(event_ms "$log" archive_trust_reuse)" \
        "$(event_ms "$log" source_reuse)" \
        "$(event_ms "$log" vendor_link)" \
        "$(event_ms "$log" autoload_write)" \
        "$(event_ms "$log" lockfile_install)" >>"$output"
}

print_stats() {
    local label="$1"
    local file="$2"

    echo "$label"
    printf '  total                 %s\n' "$(stat_col "$file" 1)"
    printf '  platform_current      %s\n' "$(stat_col "$file" 2)"
    printf '  lockfile_sources      %s\n' "$(stat_col "$file" 3)"
    printf '  archive_hash_reuse    %s\n' "$(stat_col "$file" 4)"
    printf '  archive_trust_reuse   %s\n' "$(stat_col "$file" 5)"
    printf '  source_reuse          %s\n' "$(stat_col "$file" 6)"
    printf '  vendor_link           %s\n' "$(stat_col "$file" 7)"
    printf '  autoload_write        %s\n' "$(stat_col "$file" 8)"
    printf '  lockfile_install      %s\n' "$(stat_col "$file" 9)"
}

project="$WORKDIR/project"
strict="$WORKDIR/strict.tsv"
unsafe="$WORKDIR/unsafe.tsv"

cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null
mkdir -p "$project"
cat >"$project/composer.json" <<'JSON'
{
  "require": {
    "monolog/monolog": "^3.0",
    "brick/math": "^0.14",
    "guzzlehttp/guzzle": "^7.0",
    "ramsey/uuid": "^4.0",
    "league/flysystem": "^3.0"
  }
}
JSON

(cd "$project" && "$CONCERTO_BIN" install >/dev/null)
packages="$(find "$project/vendor" -mindepth 2 -maxdepth 2 \( -type d -o -type l \) | wc -l | tr -d ' ')"

for _ in $(seq 1 "$ITERATIONS"); do
    record_run "$project" "$strict"
done

for _ in $(seq 1 "$ITERATIONS"); do
    record_run "$project" "$unsafe" --unsafe-trust-store
done

echo "Concerto local relink benchmark"
echo "case=multi-app packages=$packages iterations=$ITERATIONS"
print_stats "strict relink" "$strict"
print_stats "unsafe trusted relink" "$unsafe"
