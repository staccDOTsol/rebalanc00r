import {
  getOrCreateRandomnessServiceState,
  loadSwitchboard,
  nativeMint,
  prettyLog,
} from "../sdk";
import type { SolanaRandomnessService } from "../target/types/solana_randomness_service";

import * as anchor from "@coral-xyz/anchor";
import { bs58 } from "@switchboard-xyz/common";
import {
  FunctionAccount,
  FunctionServiceAccount,
  loadKeypair,
  SwitchboardProgram,
  TransactionObject,
} from "@switchboard-xyz/solana.js";
import chalk from "chalk";
import dotenv from "dotenv";
dotenv.config();

(async () => {
  console.log(
    `\n${chalk.green(
      "This script will close a request account using the authority wallet as a signer. Useful for closing stuck requests."
    )}`
  );

  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

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

  const state = await randomnessService.account.state.fetch(programStatePubkey);

  if (!payer.publicKey.equals(state.authority)) {
    throw new Error("Incorrect program authority");
  }

  const requestAccounts = await provider.connection.getProgramAccounts(
    randomnessService.programId,
    {
      filters: [
        {
          memcmp: {
            offset: 0,
            bytes: bs58.encode(
              Buffer.from([244, 231, 228, 160, 148, 28, 17, 184])
            ),
          },
        },
      ],
    }
  );

  if (requestAccounts.length === 0) {
    console.log("No requests found");
    return;
  }

  prettyLog(`Found ${requestAccounts.length} requests to close`, undefined, [
    "info",
  ]);

  const coder = new anchor.BorshAccountsCoder(randomnessService.idl);
  const ixns: anchor.web3.TransactionInstruction[] = [];
  for (const account of requestAccounts) {
    const request = coder.decode("RandomnessRequest", account.account.data);
    ixns.push(
      await randomnessService.methods
        .closeRequestOverride()
        .accounts({
          user: request.user,
          request: account.pubkey,
          escrow: request.escrow,
          state: programStatePubkey,
          wallet: state.wallet,
          authority: payer.publicKey,
        })
        .instruction()
    );
  }

  const txns = TransactionObject.packIxns(payer.publicKey, ixns);
  prettyLog(`Ready to send ${txns.length} transactions`, undefined, ["info"]);

  const signatures = await switchboard.signAndSendAll(txns);
  for (const sig of signatures) {
    console.log(`https://explorer.solana.com/tx/${sig}?cluster=devnet`);
  }
})();
