import {
  getOrCreateRandomnessServiceState,
  loadSwitchboard,
  nativeMint,
  prettyLog,
} from "../sdk";
import type { SolanaRandomnessService } from "../target/types/solana_randomness_service";

import * as anchor from "@coral-xyz/anchor";
import {
  FunctionAccount,
  FunctionServiceAccount,
  loadKeypair,
  SwitchboardProgram,
} from "@switchboard-xyz/solana.js";
import chalk from "chalk";
import dotenv from "dotenv";
dotenv.config();

(async () => {
  console.log(
    `\n${chalk.green(
      "This script will initialize the Switchboard Randomness service and allow us to request randomness."
    )}`
  );

  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(
    process.argv.length > 2
      ? new anchor.AnchorProvider(
          provider.connection,
          new anchor.Wallet(loadKeypair(process.argv[2])),
          {}
        )
      : provider
  );
  const payer = (provider.wallet as anchor.Wallet).payer;
  prettyLog("PAYER", payer.publicKey, ["env"]);

  const randomnessService: anchor.Program<SolanaRandomnessService> =
    anchor.workspace.SolanaRandomnessService;
  prettyLog("SolanaRandomnessService", randomnessService.programId, ["env"]);

  const [programStatePubkey, psBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("STATE")],
      randomnessService.programId
    );
  prettyLog(`ProgramState`, programStatePubkey, ["env"]);

  const switchboard = await SwitchboardProgram.fromProvider(provider);
  prettyLog(`Switchboard`, switchboard.attestationProgramId, ["env"]);

  try {
    const state = await randomnessService.account.state.fetch(
      programStatePubkey
    );
    if (process.env.VERBOSE) {
      prettyLog(
        "ProgramState",
        JSON.stringify(
          {
            ...state,
            costPerByte: state.costPerByte.toNumber(), // BN.js is annoying
          },
          undefined,
          2
        ),
        ["rpc"]
      );
    }

    console.error(`ERROR: ProgramState has already been initialized!`);
    process.exit(1);
  } catch (error) {
    if (!error.message.includes("Account does not exist")) {
      throw error;
    }
  }

  prettyLog(
    `Randomness Service's Program State account does not exist, initializing...`,
    undefined,
    ["info"]
  );

  // 1. Use $SWITCHBOARD_SERVICE_PUBKEY if it's set
  if (process.env.SWITCHBOARD_SERVICE_PUBKEY) {
    prettyLog(`SwitchboardService`, process.env.SWITCHBOARD_SERVICE_PUBKEY, [
      "env",
    ]);

    const servicePubkey = new anchor.web3.PublicKey(
      process.env.SWITCHBOARD_SERVICE_PUBKEY
    );

    const tx = await randomnessService.methods
      .initialize(new anchor.BN(10_000))
      .accounts({
        state: programStatePubkey,
        wallet: anchor.utils.token.associatedAddress({
          mint: nativeMint,
          owner: programStatePubkey,
        }),
        mint: nativeMint,
        switchboardService: servicePubkey,
      })
      .rpc();

    prettyLog(`initialize`, tx, ["tx"]);

    return;
  }

  // 2. See if $SWITCHBOARD_FUNCTION_PUBKEY is set and use that to create a new service for ourselves.
  if (process.env.SWITCHBOARD_FUNCTION_PUBKEY) {
    prettyLog(`SwitchboardFunction`, process.env.SWITCHBOARD_FUNCTION_PUBKEY, [
      "env",
    ]);

    const [functionAccount] = await FunctionAccount.load(
      switchboard,
      process.env.SWITCHBOARD_FUNCTION_PUBKEY
    );

    // TODO: check if function has servicesEnabled and we are the authority

    const [serviceAccount, tx] = await FunctionServiceAccount.create(
      switchboard,
      {
        functionAccount: functionAccount,
        name: "Randomness Service",
        metadata: `Randomness Service - ${randomnessService.programId}`,
        enclaveSize: 1024,
      }
    );
    prettyLog(`service_init`, tx, ["tx"]);
    prettyLog(`SwitchboardService`, serviceAccount.publicKey, ["env"]);

    const state = await getOrCreateRandomnessServiceState(
      randomnessService,
      serviceAccount.publicKey
    );

    console.log(`Successully initialized the randomness service`);
    process.exit(0);
  }

  // 3. Check if $SWITCHBOARD_ATTESTATION_QUEUE_PUBKEY is set

  // 4. Create a brand new environment with Queue, Verifier, Function, Service, & ServiceWorker

  throw new Error(`Failed to initialize the randomness service`);
})();
