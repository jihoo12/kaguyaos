#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source tools/jihoo-env.sh
if [[ $# == 0 ]]; then set -- init ls cat write rm ping wget; fi
bash user/build-jihoo.sh "$@"
