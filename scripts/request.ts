import { nativeMint } from "../sdk";
import type { SolanaRandomnessConsumer } from "../target/types/solana_randomness_consumer";
import type { SolanaRandomnessService } from "../target/types/solana_randomness_service";

import * as anchor from "@coral-xyz/anchor";
import { promiseWithTimeout, sleep } from "@switchboard-xyz/common";
import { loadKeypair } from "@switchboard-xyz/solana.js";
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
  console.log(`SolanaRandomnessConsumer: ${randomnessConsumer.programId}`);

  const randomnessService: anchor.Program<SolanaRandomnessService> =
    anchor.workspace.SolanaRandomnessService;
  console.log(`SolanaRandomnessService: ${randomnessService.programId}`);

  const payer = (provider.wallet as anchor.Wallet).payer;
  console.log(`[env] PAYER: ${payer.publicKey}`);

  const [programStatePubkey, psBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("STATE")],
      randomnessService.programId
    );

  try {
    const state = await randomnessService.account.state.fetch(
      programStatePubkey
    );
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

    const switchboardServicePubkey = new anchor.web3.PublicKey(
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
        switchboardService: switchboardServicePubkey,
      })
      .rpc();
    console.log("[TX] initialize", tx);
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
        "RandomnessFulfilled",
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
