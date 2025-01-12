# syntax=docker/dockerfile:1.4
FROM switchboardlabs/gramine:dev AS base

RUN mkdir -p /opt/intel/sgx-dcap-pccs/ssl_key && \
    mkdir -p /data/protected_files

WORKDIR /home/root/switchboard

###############################################################
### Build Switchboard Service
###############################################################
FROM base AS builder

WORKDIR /home/root/switchboard
# Copy anchor program
COPY ./programs/solana-randomness-service/Xargo.toml ./programs/solana-randomness-service/Xargo.toml
COPY ./programs/solana-randomness-service/Cargo.toml ./programs/solana-randomness-service/Cargo.toml
COPY ./programs/solana-randomness-service/src \
     ./programs/solana-randomness-service/src/

# Copy macros
COPY ./crates/solana-randomness-service-macros/Cargo.toml ./crates/solana-randomness-service-macros/Cargo.toml
COPY ./crates/solana-randomness-service-macros/src \
     ./crates/solana-randomness-service-macros/src/

# Copy service
COPY ./switchboard-service/Cargo.toml ./switchboard-service/Cargo.toml
COPY ./switchboard-service/Cargo.lock ./switchboard-service/Cargo.lock
COPY ./switchboard-service/src \
     ./switchboard-service/src/

RUN mkdir -p ./raydium-clmm/programs/amm 

COPY ./raydium-clmm/programs/amm/Cargo.toml ./raydium-clmm/programs/amm/Cargo.toml
COPY ./raydium-clmm/programs/amm/src \
     ./raydium-clmm/programs/amm/src/

WORKDIR /home/root/switchboard/switchboard-service

RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/home/root/switchboard/switchboard-service/target \
    cargo build && \
    mv target/debug/solana-randomness-worker /app/worker


###############################################################
### Copy to final image
###############################################################
FROM switchboardlabs/gramine

# Can be overwritten by mounting a volume
RUN mkdir -p /data/protected_files

WORKDIR /app

COPY --from=builder /app/worker /app/worker

COPY ./switchboard-service/configs/worker.manifest.template /app/worker.manifest.template
COPY ./switchboard-service/configs/boot.sh /boot.sh

RUN gramine-manifest /app/worker.manifest.template > /app/worker.manifest
RUN gramine-sgx-gen-private-key
RUN gramine-sgx-sign --manifest /app/worker.manifest --output /app/worker.manifest.sgx | tail -2 | tee /measurement.txt

RUN chmod a+x /boot.sh

# HEALTHCHECK --start-period=30s --interval=30s --timeout=3s \
#      CMD curl -f http://0.0.0.0:8080/healthz || exit 1

ENTRYPOINT bash /boot.sh
