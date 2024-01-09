#!/bin/bash

if [[ "${UID}" -ne 0 ]]; then
    echo "Please run this script with root privileges."
fi

function verify_aesm_service() {
  if pgrep aesm_service > /dev/null; then
      return 0
  else
      return 1
  fi
}

if ! verify_aesm_service; then
  echo "Error: aesm_service is not running"

  echo "Running /restart_aesm.sh"
  (
    /restart_aesm.sh
  )

  # exit 1
fi

echo "Starting enclave.."
gramine-sgx /app/worker
