#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
source tools/jihoo-env.sh
bash user/build.sh
bash user/insert.sh
exec bash run.sh "$@"
