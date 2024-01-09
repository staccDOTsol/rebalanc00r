#!/bin/bash

if [[ "${UID}" -ne 0 ]]; then
    echo "Please run this script with root privileges."
fi

function verify_aesm_service() {
  local retries=0
  while [[ ${retries} -lt 5 ]]; do
    if pgrep aesm_service > /dev/null; then
      echo "aesm_service is running."
      return 0
    else
      echo "Error: aesm_service is not running. Retrying in 1 second..."
      sleep 1
      ((retries++))
    fi
  done
  return 1
}

if ! verify_aesm_service; then
  echo "Error: aesm_service is not running"
  exit 1
fi

echo "Starting enclave.."
gramine-sgx /app/worker
