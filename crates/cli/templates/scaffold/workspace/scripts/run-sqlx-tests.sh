#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "用法: $0 <config/server.toml> <测试命令> [参数...]" >&2
    exit 2
fi

config_path="$1"
shift

database_url="$({
    python3 - "$config_path" <<'PY'
import pathlib
import sys
import tomllib

config_path = pathlib.Path(sys.argv[1])
with config_path.open("rb") as config_file:
    config = tomllib.load(config_file)

url = config.get("database", {}).get("url")
if not isinstance(url, str) or not url:
    raise SystemExit(f"{config_path} 缺少非空 database.url")

print(url)
PY
})"

export DATABASE_URL="$database_url"

before_snapshot="$(mktemp "${TMPDIR:-/tmp}/nexora-sqlx-before.XXXXXX")"
after_snapshot="$(mktemp "${TMPDIR:-/tmp}/nexora-sqlx-after.XXXXXX")"
new_snapshot="$(mktemp "${TMPDIR:-/tmp}/nexora-sqlx-new.XXXXXX")"

cleanup_files() {
    rm -f "$before_snapshot" "$after_snapshot" "$new_snapshot"
}
trap cleanup_files EXIT

snapshot_test_databases() {
    if [[ "$(psql "$DATABASE_URL" -X -Atc "SELECT to_regclass('_sqlx_test.databases') IS NOT NULL")" == "t" ]]; then
        psql "$DATABASE_URL" -X -Atc \
            "SELECT db_name || E'\\t' || created_at::TEXT FROM _sqlx_test.databases ORDER BY db_name, created_at"
    fi
}

snapshot_test_databases >"$before_snapshot"

set +e
"$@"
command_status=$?
set -e

snapshot_test_databases >"$after_snapshot"
comm -13 "$before_snapshot" "$after_snapshot" >"$new_snapshot"

while IFS=$'\t' read -r database_name _created_at; do
    [[ -n "$database_name" ]] || continue
    if [[ ! "$database_name" =~ ^_sqlx_test_[A-Za-z0-9_]{52}$ ]]; then
        echo "拒绝清理无法识别的 SQLx 测试数据库名" >&2
        exit 1
    fi

    test_database_url="$(BASE_DATABASE_URL="$DATABASE_URL" TEST_DATABASE_NAME="$database_name" python3 - <<'PY'
import os
import urllib.parse

base = urllib.parse.urlsplit(os.environ["BASE_DATABASE_URL"])
test_path = "/" + os.environ["TEST_DATABASE_NAME"]
print(urllib.parse.urlunsplit((base.scheme, base.netloc, test_path, base.query, base.fragment)))
PY
)"
    sqlx database drop --no-dotenv -y -D "$test_database_url" >/dev/null
    psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 \
        -c "DELETE FROM _sqlx_test.databases WHERE db_name = '$database_name'" >/dev/null
done <"$new_snapshot"

exit "$command_status"
