#!/usr/bin/env bash
set -euo pipefail
: "${WHORL_ACCEPTANCE_ADMIN_URL:?set a disposable PostgreSQL admin URL}"
: "${WHORL_STORAGE_S3_ENDPOINT:?set the MinIO endpoint with an existing uploads bucket}"
: "${AWS_ACCESS_KEY_ID:?set MinIO credentials}"
: "${AWS_SECRET_ACCESS_KEY:?set MinIO credentials}"
profile=(--profile test)
if [[ ${WHORL_ACCEPTANCE_RELEASE:-0} == 1 ]]; then profile=(--release); fi
prefix="whorl_acceptance_$(date +%s)"
export WHORL_OPERATIONS_S3_ENDPOINT="$WHORL_STORAGE_S3_ENDPOINT"
export WHORL_E2E_S3_ENDPOINT="$WHORL_STORAGE_S3_ENDPOINT"
for entry in schema:SCHEMA candidates:CANDIDATES worker:WORKER storage:STORAGE admission:ADMISSION admission:ADMISSION_UPLOAD enqueue:ENQUEUE uploads:UPLOAD polling:POLL maintenance:MAINTENANCE operations:OPERATIONS end_to_end:E2E; do
  if [[ $entry == end_to_end:* && $(uname -s) != Linux ]]; then
    printf "%s\n" "SKIP end_to_end: successful extraction requires Linux"
    continue
  fi
  suite=${entry%%:*}
  setting=${entry##*:}
  database="${prefix}_$(printf %s "$setting" | tr A-Z a-z)"
  psql -X -w "$WHORL_ACCEPTANCE_ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE $database"
  export "WHORL_${setting}_TEST_URL=${WHORL_ACCEPTANCE_ADMIN_URL%/*}/$database"
  if [[ $setting == ADMISSION ]]; then
    cargo test --locked "${profile[@]}" --test "$suite" -- --ignored --exact races_isolation_expiry_and_idempotent_release
  elif [[ $setting == ADMISSION_UPLOAD ]]; then
    cargo test --locked "${profile[@]}" --test "$suite" -- --ignored --exact upload_transfer_reconciles_bytes_and_bounds_file_attempts
  else
    cargo test --locked "${profile[@]}" --test "$suite" -- --ignored --test-threads=1 --nocapture
  fi
done
