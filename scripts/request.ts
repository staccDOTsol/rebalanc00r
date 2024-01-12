import type { State } from "../sdk";
import { nativeMint, prettyLog } from "../sdk";
import type { SolanaRandomnessConsumer } from "../target/types/solana_randomness_consumer";
import type { SolanaRandomnessService } from "../target/types/solana_randomness_service";

import * as anchor from "@coral-xyz/anchor";
import { promiseWithTimeout, sleep } from "@switchboard-xyz/common";
import {
  FunctionServiceAccount,
  loadKeypair,
  NativeMint,
  SwitchboardProgram,
} from "@switchboard-xyz/solana.js";
import chalk from "chalk";
import dotenv from "dotenv";
dotenv.config();

interface RandomnessFulfilled {
  request: anchor.web3.PublicKey;
  isSuccess: boolean;
  randomness: Buffer;
}

(async () => {
  console.log(
    `\n${chalk.green(
      "This script will request randomness from the Solana Randomness Service. Upon fulfillment, the randomness service will call our consumer program with the randomness bytes"
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

  const randomnessConsumer: anchor.Program<SolanaRandomnessConsumer> =
    anchor.workspace.SolanaRandomnessConsumer;
  prettyLog(`SolanaRandomnessConsumer`, randomnessConsumer.programId, ["env"]);

  const randomnessService: anchor.Program<SolanaRandomnessService> =
    anchor.workspace.SolanaRandomnessService;
  prettyLog(`SolanaRandomnessService`, randomnessService.programId, ["env"]);

  const payer = (provider.wallet as anchor.Wallet).payer;
  prettyLog("PAYER", payer.publicKey, ["env"]);

  const [programStatePubkey, psBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("STATE")],
      randomnessService.programId
    );
  prettyLog(`ProgramState`, programStatePubkey, ["env"]);

  const switchboard = await SwitchboardProgram.fromProvider(provider);
  prettyLog(`Switchboard`, switchboard.attestationProgramId, ["env"]);

  let state: State | undefined = undefined;

  try {
    state = await randomnessService.account.state.fetch(programStatePubkey);
    if (process.env.VERBOSE) {
      console.log(
        `[STATE] ${JSON.stringify(
          {
            ...state,
            costPerByte: state.costPerByte.toNumber(), // BN.js is annoying
          },
          undefined,
          2
        )}`
      );
    }
  } catch (error) {
    if (!error.message.includes("Account does not exist")) {
      throw error;
    }

    console.log(
      `[STATE] ${programStatePubkey.toBase58()} does not exist, initializing...`
    );

    if (!process.env.SWITCHBOARD_SERVICE_PUBKEY) {
      throw new Error(
        `Please set SWITCHBOARD_SERVICE_PUBKEY to initialize the randomness service program state`
      );
    }

    const [serviceAccount, serviceState] = await FunctionServiceAccount.load(
      switchboard,
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
        switchboardService: serviceAccount.publicKey,
        switchboardFunction: serviceState.function,
      })
      .rpc();
    console.log("[TX] initialize", tx);

    state = {
      bump: psBump,
      authority: payer.publicKey,
      mint: NativeMint.address,
      switchboardService: serviceAccount.publicKey,
      wallet: anchor.utils.token.associatedAddress({
        mint: nativeMint,
        owner: programStatePubkey,
      }),
      costPerByte: new anchor.BN(10_000),
      lastUpdated: new anchor.BN(0),
      ebuf: [],
    };
  }

  const request = anchor.web3.Keypair.generate();

  const config = { commitment: "confirmed" } as const;

  const [randomnessServiceEventAuthority] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("__event_authority")],
      randomnessService.programId // uses randomness service pid
    );

  let listener: number | undefined = undefined;

  const closeListener = async () => {
    if (listener !== null) {
      await randomnessService.removeEventListener(listener);
      listener = null;
    }
  };

  const callbackPromise = new Promise(
    async (
      resolve: (value: [RandomnessFulfilled, number]) => void,
      _reject
    ) => {
      listener = randomnessService.addEventListener(
        "RandomnessFulfilledEvent",
        (event, slot) => {
          resolve([event, slot]);
        }
      );
    }
  );

  // keep getting blockhash issues, so retry
  let tx: string | undefined = undefined;
  const retryCount = 5;
  while (retryCount > 0) {
    try {
      tx = await randomnessConsumer.methods
        .requestRandomness()
        .accounts({
          randomnessRequest: request.publicKey,
          randomnessEscrow: anchor.utils.token.associatedAddress({
            mint: nativeMint,
            owner: request.publicKey,
          }),
          randomnessState: programStatePubkey,
          randomnessMint: nativeMint,
          randomnessService: randomnessService.programId,
        })
        .signers([request])
        .rpc(config);
      console.log(
        `[TX] consumer requests randomness https://explorer.solana.com/tx/${tx}?cluster=devnet`
      );
      prettyLog("RequestAccount", request.publicKey, ["info"]);
      break;
    } catch (error) {
      console.log(`[TX] error: ${error.message}`);
      if (error.message.includes("Blockhash not found")) {
        console.log(`Retrying...`);
        retryCount - 1;
        sleep(100);
        continue;
      } else {
        throw error;
      }
    }
  }

  if (!tx) {
    throw new Error(`Failed to request randomness`);
  }

  const [event, slot] = await callbackPromise;

  console.log(
    `[EVENT] ${JSON.stringify(
      { ...event, randomness: `[${new Uint8Array(event.randomness)}]` },
      undefined,
      2
    )}`
  );

  // need to fetch the sig so we know the slot it was requested at
  const requestSlot = (
    await provider.connection.getTransaction(tx, {
      commitment: "confirmed",
    })
  ).slot;

  console.log(
    `\n > ${chalk.green(`Request took ${slot - requestSlot} slots.`)}`
  );

  await closeListener();
})();
