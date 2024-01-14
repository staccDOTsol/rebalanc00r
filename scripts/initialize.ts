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


import { BN, parseRawMrEnclave } from "@switchboard-xyz/common";
import type { attestationTypes } from "@switchboard-xyz/solana.js";
import { SwitchboardWallet, VerifierAccount } from "@switchboard-xyz/solana.js";
import {
  AttestationQueueAccount,
} from "@switchboard-xyz/solana.js";
import dotenv from "dotenv";
dotenv.config();


export function getSwitchboardWalletPubkeys(
  program: anchor.Program,
  attestationQueue: anchor.web3.PublicKey,
  authority: anchor.web3.PublicKey,
  name?: string | anchor.web3.PublicKey
): [anchor.web3.PublicKey, anchor.web3.PublicKey] {
  const rawNameBytes: Uint8Array =
    name instanceof anchor.web3.PublicKey
      ? name.toBytes()
      : new Uint8Array(Buffer.from(name ?? "DefaultWallet"));

  const nameBytes = new Uint8Array(32);
  nameBytes.set(rawNameBytes);

  const escrowWalletPubkey = anchor.web3.PublicKey.findProgramAddressSync(
    [
      nativeMint.toBytes(),
      attestationQueue.toBytes(),
      authority.toBytes(),
      nameBytes.slice(0, 32),
    ],
    program.programId
  )[0];

  const escrowTokenWalletPubkey = anchor.utils.token.associatedAddress({
    owner: escrowWalletPubkey,
    mint: nativeMint,
  });

  return [escrowWalletPubkey, escrowTokenWalletPubkey];
}
export function logEnvVariables(
  env: Array<[string, string | anchor.web3.PublicKey]>,
  pre = "Make sure to add the following to your .env file:"
) {
  console.log(
    `\n${pre}\n\t${env
      .map(
        ([key, value]) =>
          `${chalk.blue(key.toUpperCase())}=${chalk.yellow(value)}`
      )
      .join("\n\t")}\n`
  );
}

(async () => {
  console.log(
    `\n${chalk.green(
      "This script will initialize the Switchboard Reblancing service and allow us to request randomness."
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

  const switchboard = await SwitchboardProgram.fromProvider(provider);
  prettyLog(`Switchboard`, switchboard.attestationProgramId, ["env"]);





  const program = await switchboard.attestationProgram;

  const switchboardProgram = new SwitchboardProgram(
    provider,
    undefined,
    undefined,
    program.programId,
    undefined,
    Promise.resolve(program) as Promise<anchor.Program<anchor.Idl>>
  );


  const [programStatePubkey] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("STATE")],
    program.programId
  );
  console.log(`PROGRAM_STATE: ${programStatePubkey}`);


  let attestationQueueAccount = new anchor.web3.PublicKey("CkvizjVnm2zA5Wuwan34NhVT3zFc7vqUyGnA6tuEF5aE")

  let verifierOracleAccount: VerifierAccount | undefined = undefined;

  let serviceWorkerPubkey: anchor.web3.PublicKey | undefined = undefined;

  let [functionAccount] = await FunctionAccount.load(
    switchboard,
    new anchor.web3.PublicKey(process.env.SWITCHBOARD_FUNCTION_PUBKEY)
  )
  /////////////////////////////////////////
  // GET OR CREATE CREATE PROGRAM STATE  //
  /////////////////////////////////////////



  console.log(`Program state not found, initializing ...`);


  /////////////////////////////////////////////
  // GET OR CREATE CREATE ATTESTATION QUEUE  //
  /////////////////////////////////////////////


  /////////////////////////////////////////////
  // GET OR CREATE CREATE SERVICE WORKER     //
  /////////////////////////////////////////////
  try {
    if (process.env.SWITCHBOARD_SERVICE_WORKER_KEY) {
      const serviceWorkerState =
        await program.account.serviceWorkerAccountData.fetch(
          new anchor.web3.PublicKey(process.env.SWITCHBOARD_SERVICE_WORKER_KEY)
        );

      serviceWorkerPubkey = new anchor.web3.PublicKey(
        process.env.SWITCHBOARD_SERVICE_WORKER_KEY
      );
    }
  } catch { }

  if (true) {
    console.log(`ServiceWorker not found, initializing ...`);
    /*
        const serviceWorkerKeypair = anchor.web3.Keypair.generate();
    
        const [rewardEscrowWallet, rewardEscrowTokenWallet] =
          getSwitchboardWalletPubkeys(
            program,
            attestationQueueAccount,
            payer.publicKey,
            serviceWorkerKeypair.publicKey
          );
    
        const tx = await program.methods
          .serviceWorkerInit({
            region: { unitedKingdom: {} },
            zone: { south: {} },
            permissionsRequired: false,
            availableEnclaveSize: new BN(10 * 1024 * 1024),
            maxEnclaveSize: new BN(1 * 1024 * 1024),
            maxCpu: new BN(1),
            enclaveCost: new BN(0),
            maxServicesLen: 16,
          })
          .accounts({
            serviceWorker: serviceWorkerKeypair.publicKey,
            authority: payer.publicKey,
    
            attestationQueue: attestationQueueAccount,
    
            rewardEscrowWallet: rewardEscrowWallet,
            rewardEscrowTokenWallet: rewardEscrowTokenWallet,
            rewardEscrowWalletAuthority: null,
            mint: switchboardProgram.mint.address,
    
            payer: payer.publicKey,
          })
          .signers([serviceWorkerKeypair])
          .rpc(); */
    let serviceWorkerPubkey = new anchor.web3.PublicKey(process.env.SWITCHBOARD_SERVICE_WORKER_KEY)
    const serviceWorker =
      await program.account.serviceWorkerAccountData.fetch(
        serviceWorkerPubkey///serviceWorkerKeypair.publicKey
      );

    //serviceWorkerPubkey = serviceWorkerKeypair.publicKey;

    logEnvVariables([
      ["SWITCHBOARD_SERVICE_WORKER_KEY", serviceWorkerPubkey.toBase58()],
    ]);

    // console.log(`[TX] serviceWorkerInit: ${tx}`);

    /////////////////////////////////////////////
    // CREATE SERVICE ACCOUNT                  //
    /////////////////////////////////////////////
    let servicePubkey: anchor.web3.PublicKey | undefined = undefined;
    const serviceKeypair = anchor.web3.Keypair.generate();
    servicePubkey = serviceKeypair.publicKey;
    const [defaultSbWallet, escrowTokenWallet] = getSwitchboardWalletPubkeys(
      program,
      attestationQueueAccount,
      payer.publicKey,
      serviceKeypair.publicKey
    );
    const serviceInit = await program.methods
      .functionServiceInit({
        name: Buffer.from("Reblancing Service"),
        metadata: Buffer.from("switchboard rebalacing service"),
        enclaveSize: new BN(1 * 1024 * 1024),
        cpu: new BN(1),
        maxContainerParamsLen: 1024,
        containerParams: Buffer.from(""),
      })
      .accounts({
        service: servicePubkey,
        authority: payer.publicKey,
        function: functionAccount.publicKey,
        functionAuthority: null,
        escrowWallet: defaultSbWallet,
        escrowTokenWallet: escrowTokenWallet,
        escrowWalletAuthority: null,
        mint: nativeMint,
        attestationQueue: attestationQueueAccount,
        payer: payer.publicKey,
      })
      .signers([serviceKeypair])
      .rpc();
    console.log(`[TX] serviceInit: ${serviceInit}`);

    logEnvVariables([["SWITCHBOARD_SERVICE_KEY", servicePubkey.toBase58()]]);

    const addServiceSignature = await program.methods
      .functionServiceAddWorker({})
      .accounts({
        serviceWorker: serviceWorkerPubkey,
        service: servicePubkey,
        function: functionAccount.publicKey,
        authority: payer.publicKey,
      })
      .rpc();
    console.log(`[TX] functionServiceAddWorker: ${addServiceSignature}`);

    // testMeter.print();

    logEnvVariables([
      [
        "SWITCHBOARD_ATTESTATION_QUEUE_KEY",
        attestationQueueAccount.toBase58(),
      ],
      ["SWITCHBOARD_VERIFIER_ORACLE_KEY", verifierOracleAccount.publicKey],

      ["SWITCHBOARD_SERVICE_WORKER_KEY", serviceWorkerPubkey.toBase58()],

      ["SWITCHBOARD_FUNCTION_KEY", functionAccount.publicKey.toBase58()],
      ["SWITCHBOARD_SERVICE_KEY", servicePubkey.toBase58()],
    ]);
  }
})();
