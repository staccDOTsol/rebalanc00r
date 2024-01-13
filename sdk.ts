import type { SolanaRandomnessConsumer } from "./target/types/solana_randomness_consumer";
import type { SolanaRandomnessService } from "./target/types/solana_randomness_service";

import * as anchor from "@coral-xyz/anchor";
import type { IdlEvent, IdlEventField } from "@coral-xyz/anchor/dist/cjs/idl";
import { promiseWithTimeout, sleep } from "@switchboard-xyz/common";
import type { BootstrappedAttestationQueue } from "@switchboard-xyz/solana.js";
import {
  AttestationQueueAccount,
  attestationTypes,
  DEVNET_GENESIS_HASH,
  FunctionAccount,
  FunctionServiceAccount,
  MAINNET_GENESIS_HASH,
  ServiceWorkerAccount,
  SwitchboardProgram,
  SwitchboardWallet,
} from "@switchboard-xyz/solana.js";
import chalk from "chalk";
import dotenv from "dotenv";
dotenv.config();

export interface State {
  bump: number;
  authority: anchor.web3.PublicKey;
  mint: anchor.web3.PublicKey;
  switchboardService: anchor.web3.PublicKey;
  wallet: anchor.web3.PublicKey;
  costPerByte: anchor.BN;
  lastUpdated: anchor.BN;
  ebuf: Array<number>;
}

export interface RandomnessRequest {
  isCompleted: number;
  numBytes: number;
  user: anchor.web3.PublicKey;
  escrow: anchor.web3.PublicKey;
  requestSlot: anchor.BN;
  callback: {
    programId: anchor.web3.PublicKey;
    accounts: Array<anchor.web3.AccountMeta>;
    data: Buffer;
  };
  errorMessage: string;
}

export const prettyLog = (label: string, value?: any, tags?: string[]) => {
  const getTagString = (t: string) => {
    const tag = "[" + t + "]";

    switch (t.toLowerCase()) {
      case "tx":
        return chalk.green(tag);
      case "error":
        return chalk.red(tag);
      case "info":
      case "rpc":
        return chalk.blue(tag);
      case "env":
        return chalk.yellow(tag);
      default:
        return tag;
    }
  };

  const tagString = tags.reduce((str, tag) => {
    return str + getTagString(tag);
  }, "");

  if (value) {
    console.log(`${tagString} ${chalk.bold(label)}: ${value}`);
  } else {
    console.log(`${tagString} ${label}`);
  }
};

export const nativeMint: anchor.web3.PublicKey = new anchor.web3.PublicKey(
  "So11111111111111111111111111111111111111112"
);

export const PLUS_ICON = chalk.green("\u002B ");

export const CHECK_ICON = chalk.green("\u2714 ");

export const FAILED_ICON = chalk.red("\u2717 ");

export interface RandomnessFulfilledEvent {
  request: anchor.web3.PublicKey;
  isSuccess: boolean;
  randomness: Buffer;
}

/** Returns whether we're connected to a localnet cluster */
export async function isLocalnet(
  connection: anchor.web3.Connection
): Promise<boolean> {
  if (
    connection.rpcEndpoint.includes("localhost") ||
    connection.rpcEndpoint.includes("0.0.0.0")
  ) {
    return true;
  }

  const genesisHash = await connection.getGenesisHash();
  return (
    genesisHash !== DEVNET_GENESIS_HASH && genesisHash !== MAINNET_GENESIS_HASH
  );
}

export async function getOrCreateRandomnessServiceState(
  randomnessService: anchor.Program<SolanaRandomnessService>,
  switchboardServicePubkey?: anchor.web3.PublicKey
): Promise<State> {
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
    return state;
  } catch (error) {
    if (!error.message.includes("Account does not exist")) {
      throw error;
    }

    prettyLog(
      `Randomness Service's Program State account does not exist, initializing...`,
      "",
      ["info"]
    );

    const servicePubkey =
      switchboardServicePubkey ?? process.env.SWITCHBOARD_SERVICE_PUBKEY
        ? new anchor.web3.PublicKey(process.env.SWITCHBOARD_SERVICE_PUBKEY)
        : undefined;

    if (!servicePubkey) {
      throw new Error(
        `Please set SWITCHBOARD_SERVICE_PUBKEY to initialize the randomness service program state`
      );
    }

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
        // switchboardFunction:
      })
      .rpc();

    prettyLog(`initialize`, tx, ["tx"]);

    return {
      bump: psBump,
      authority: (randomnessService.provider as anchor.AnchorProvider).wallet
        .publicKey,
      mint: nativeMint,
      switchboardService: servicePubkey,
      wallet: anchor.utils.token.associatedAddress({
        mint: nativeMint,
        owner: programStatePubkey,
      }),
      costPerByte: new anchor.BN(10_000),
      lastUpdated: new anchor.BN(0),
      ebuf: [],
    };
  }
}

/// Loads the Switchboard program from environment variables. If not provided or found, a brand new environment
/// will be configured.
export async function loadSwitchboard(
  provider: anchor.AnchorProvider
): Promise<[SwitchboardProgram, FunctionAccount, FunctionServiceAccount]> {
  const switchboard = await SwitchboardProgram.fromProvider(provider);
  prettyLog(`Switchboard`, switchboard.attestationProgramId, ["env"]);

  // First, check if the env var SWITCHBOARD_SERVICE_PUBKEY is set. If so, load it and check if it exists on-chain.
  if (process.env.SWITCHBOARD_SERVICE_PUBKEY) {
    prettyLog(
      `SWITCHBOARD_SERVICE_PUBKEY`,
      process.env.SWITCHBOARD_SERVICE_PUBKEY,
      ["env"]
    );
    const [serviceAccount, serviceState] = await FunctionServiceAccount.load(
      switchboard,
      process.env.SWITCHBOARD_SERVICE_PUBKEY
    );

    // Verify function exists on-chain
    const [functionAccount, functionState] = await FunctionAccount.load(
      switchboard,
      serviceState.function
    );

    return [switchboard, functionAccount, serviceAccount];
  }

  // // Next, check if the env var SWITCHBOARD_FUNCTION_PUBKEY is set. If so, load it and check if it exists on-chain.
  // if (process.env.SWITCHBOARD_FUNCTION_PUBKEY) {
  //   prettyLog(
  //     `SWITCHBOARD_FUNCTION_PUBKEY`,
  //     process.env.SWITCHBOARD_FUNCTION_PUBKEY,
  //     ["env"]
  //   );

  //   const [functionAccount, functionState] = await FunctionAccount.load(
  //     switchboard,
  //     process.env.SWITCHBOARD_FUNCTION_PUBKEY
  //   );
  // }

  // Next, check if the env var SWITCHBOARD_ATTESTATION_QUEUE_PUBKEY is set. If so, load it and check if it exists on-chain.

  // Next, check if the env var SWITCHBOARD_SERVICE_WORKER_PUBKEY is set. If so, load it and check if it exists on-chain.

  // If here, create a new bootstrapped attestation queue and service worker.

  throw new Error("Not implemented");
}

export async function createSwitchboardService(
  provider: anchor.AnchorProvider
): Promise<
  [
    SwitchboardProgram,
    FunctionAccount,
    FunctionServiceAccount,
    BootstrappedAttestationQueue
  ]
> {
  const switchboard = await SwitchboardProgram.fromProvider(provider);
  prettyLog(`Switchboard`, switchboard.attestationProgramId, ["env"]);

  const payer = switchboard.provider.wallet.publicKey;
  prettyLog(`Payer`, payer, ["env"]);

  const bootstrappedQueue = await AttestationQueueAccount.bootstrapNewQueue(
    switchboard
  );
  prettyLog(`AttestationQueue`, bootstrappedQueue.attestationQueue.publicKey, [
    "env",
  ]);

  const [serviceWorkerAccount] = await ServiceWorkerAccount.create(
    switchboard,
    {
      attestationQueue: bootstrappedQueue.attestationQueue.publicKey,
      region: new attestationTypes.ServerRegion.UnitedStates(),
      zone: new attestationTypes.ServerZone.West(),
      maxEnclaveSize: 10 * 1024 * 1024,
      availableEnclaveSize: 10 * 1024 * 1024,
      maxCpu: 1,
      maxServices: 10,
      enclaveCost: 0,
    }
  );
  prettyLog(`ServiceWorker`, serviceWorkerAccount.publicKey, ["env"]);

  // Use a common SbWallet for all of our functions/services so we only need to fund one
  let sbWallet: SwitchboardWallet;
  try {
    sbWallet = SwitchboardWallet.fromSeed(
      switchboard,
      bootstrappedQueue.attestationQueue.publicKey,
      payer,
      "default"
    );
    await sbWallet.loadData();
  } catch (error) {
    [sbWallet] = await SwitchboardWallet.create(
      switchboard,
      bootstrappedQueue.attestationQueue.publicKey,
      payer,
      "default",
      16
    );
  }
  prettyLog(`SwitchboardWallet`, sbWallet.publicKey, ["env"]);

  const [functionAccount] =
    await bootstrappedQueue.attestationQueue.account.createFunction(
      {
        container: "N/A",
      },
      sbWallet
    );
  prettyLog(`FunctionAccount`, functionAccount.publicKey, ["env"]);

  const [serviceAccount] = await FunctionServiceAccount.create(
    switchboard,
    {
      functionAccount,
      enclaveSize: 1 * 1024 * 1024,
    },
    sbWallet
  );
  prettyLog(`ServiceAccount`, serviceAccount.publicKey, ["env"]);

  // TODO: have the service request a new enclaveSigner and have the bootstrapped queue approve it

  return [switchboard, functionAccount, serviceAccount, bootstrappedQueue];
}

export async function printLogs(
  connection: anchor.web3.Connection,
  tx: string,
  v0Txn: boolean = false,
  delay = 3000
) {
  if (delay > 0) {
    await sleep(delay);
  }

  const parsed = await connection.getParsedTransaction(tx, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: v0Txn ? 0 : undefined,
  });
  console.log(parsed?.meta?.logMessages?.join("\n"));
}

export const runAndAwaitEvent = async <I extends anchor.Idl>(
  program: anchor.Program<I>,
  eventName: keyof anchor.IdlEvents<I>,
  txnPromise: Promise<anchor.web3.TransactionSignature>
): Promise<[anchor.IdlEvents<I>[typeof eventName], number]> => {
  let listener = null;
  const closeListener = async () => {
    if (listener !== null) {
      await program.removeEventListener(listener);
      listener = null;
    }
  };

  const callbackPromise = new Promise(
    async (
      resolve: (value: [anchor.IdlEvents<I>[typeof eventName], number]) => void,
      _reject
    ) => {
      listener = program.addEventListener(eventName, (event, slot) => {
        resolve([
          /** The cast event as anchor.IdlEvents<I>[typeof eventName] is required because
           * TypeScript may not be able to infer that the event parameter in the event
           * listener callback is of the specific type corresponding to eventName. This
           * cast asserts the correct type based on eventName. */
          event as anchor.IdlEvents<I>[typeof eventName],
          slot,
        ]);
      });
      await txnPromise;
    }
  );

  const result = await promiseWithTimeout(45_000, callbackPromise);
  await closeListener();
  return result;
};

export const parseCpiEvent = async <I extends anchor.Idl>(
  program: anchor.Program<I>,
  eventName: keyof anchor.IdlEvents<I>,
  tx: anchor.web3.TransactionSignature
): Promise<anchor.EventData<IdlEventField, Record<string, string>>> => {
  // Parse the event from the transaction.
  // TODO: add retry logic
  const txResult = await program.provider.connection.getTransaction(tx, {
    commitment: "confirmed",
  });

  // The very last inner Ixn containers our event
  // TODO: find the event based on discriminator
  const innerIxn = txResult.meta.innerInstructions[0].instructions.slice(-1)[0];
  const ixData = anchor.utils.bytes.bs58.decode(innerIxn.data);
  const eventData = anchor.utils.bytes.base64.encode(ixData.slice(8));

  const event = program.coder.events.decode(eventData);

  if (!event) {
    throw new Error("Failed to yield an event");
  }

  if (event.name !== eventName) {
    throw new Error(`Expected event ${eventName} but got ${event.name}`);
  }

  const anchorEvent = event.data;

  return anchorEvent;
};
