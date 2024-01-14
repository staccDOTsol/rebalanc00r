# Solana Randomness Service

This example uses a Switchboard Service to respond to randomness requests on-chain.

## TODO

- [ ] Add scripts/functionality to bootstrap a Switchboard environment with a verifier.
- [ ] Add a spam script to use 100 keypairs to measure the servers performance.
- [x] [**RandomnessService**]: Add `update_program_config` instruction to change the SwitchboardService or modify the fees.
- [x] Add a proc macro to automatically add the Switchboard accounts to the Anchor Accounts ctx. It should also add a trait implementation which allows the user to call `ctx.request_randomness(num_bytes, callback)`. **This was added but breaks IDL generation -\_-**.
- [ ] Add the ability to support a different mint for `cost_per_byte` so a different token is used for fees.
- [ ] Add the ability to include priority fees on a request.

## Programs

| Program             | Description                                                                                                                                                                                                                                                                                                                                                               |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Randomness Service  | `RANDMo5gFnqnXJW5Z52KNmd24sAo95KAd5VbiCtq5Rh` <br />This program is responsible for receiving and settling randomness requests. Any program may invoke the randomness service and request N bytes of randomness. Each request emits an anchor event and creates a request account. Upon settlement, the request account is closed indicating the randomness was received. |
| Randomness Consumer | `39hMZgeiesFXMRFt8svuKVsdCW5geiYueSRx7dxhXN4f` <br />This is an example program of a randomness consumer which will invoke the randomness service and log the received randomness bytes.                                                                                                                                                                                  |

## Setup

First, we need to setup our program. Run the `anchor keys sync` command to update the program IDs with our local keypairs.

```bash
$ anchor keys sync
Found incorrect program id declaration in "/Users/gally/dev/switchboard/solana-randomness-service/programs/solana-randomness-consumer/src/lib.rs"
Updated to Ckz2jPvHi1nz36FRdE3x1tAjTwkLuK9UHxvreH9BPaMK

Found incorrect program id declaration in Anchor.toml for the program `solana_randomness_consumer`
Updated to Ckz2jPvHi1nz36FRdE3x1tAjTwkLuK9UHxvreH9BPaMK

Found incorrect program id declaration in "/Users/gally/dev/switchboard/solana-randomness-service/programs/solana-randomness-service/src/lib.rs"
Updated to 5g2wcfeJ8FUetws5KWdUEN1MDeqqSNis2A5iDmqrijyj

Found incorrect program id declaration in Anchor.toml for the program `solana_randomness_service`
Updated to 5g2wcfeJ8FUetws5KWdUEN1MDeqqSNis2A5iDmqrijyj

All program id declarations are synced.
```

Next, lets build the programs and test it locally.

```bash
anchor build
anchor test --provider.cluster localnet
```

Now deploy the program's and IDL to devnet:

```bash
anchor deploy --provider.cluster devnet

anchor idl init --provider.cluster devnet \
    -f target/idl/solana_randomness_service.json \
    $(solana-keygen pubkey target/deploy/solana_randomness_service-keypair.json)

anchor idl init --provider.cluster devnet \
    -f target/idl/solana_randomness_consumer.json \
    $(solana-keygen pubkey target/deploy/solana_randomness_consumer-keypair.json)
```

**IDL Upgrade:**

```bash
anchor idl upgrade --provider.cluster devnet \
    --provider.wallet ~/switchboard_environments_v2/devnet/upgrade_authority/upgrade_authority.json \
    -f target/idl/solana_randomness_service.json \
    $(solana-keygen pubkey target/deploy/solana_randomness_service-keypair.json)

anchor idl upgrade --provider.cluster devnet \
    --provider.wallet ~/switchboard_environments_v2/devnet/upgrade_authority/upgrade_authority.json \
    -f target/idl/solana_randomness_consumer.json \
    $(solana-keygen pubkey target/deploy/solana_randomness_consumer-keypair.json)
```

Now we need to initialize the Randomness Service program with our Switchboard Service pubkey:

```bash
echo "TODO"
```

To initiate a request:

```bash
SWITCHBOARD_SERVICE_KEY="2fpdEbugwThMjRQ728Ne4zwGsrjFcCtmYDnwGtzScfnL" anchor run request
```

To update the service program crate:

```bash
sb solana function sync-enclave AHV7ygefHZQ5extiZ4GbseGANg3AwBWgSUfnUktTrxjd \
    --setVersion dev-RC_01_12_24_00_07 \
    --keypair ~/switchboard_environments_v2/devnet/upgrade_authority/upgrade_authority.json \
    --force
```
