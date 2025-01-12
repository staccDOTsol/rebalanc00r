import {
  createSwitchboardService,
  loadSwitchboard,
  nativeMint,
  printLogs,
  runAndAwaitEvent,
} from "../sdk";
import type { SolanaRandomnessConsumer } from "../target/types/solana_randomness_consumer";
import type { SolanaRandomnessService } from "../target/types/solana_randomness_service";

import type { Program } from "@coral-xyz/anchor";
import * as anchor from "@coral-xyz/anchor";
import { BN, sleep } from "@switchboard-xyz/common";
import type { FunctionAccount } from "@switchboard-xyz/solana.js";
import {
  FunctionServiceAccount,
  type SwitchboardProgram,
} from "@switchboard-xyz/solana.js";
import { assert } from "chai";

describe("Solana Randomness Service", () => {
  // Configure the client to use the local cluster.
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const randomnessService = anchor.workspace
    .SolanaRandomnessService as Program<SolanaRandomnessService>;
  console.log(`randomnessService: ${randomnessService.programId}`);

  const [programStatePubkey, psBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("STATE")],
      randomnessService.programId
    );

  const consumerProgram = anchor.workspace
    .SolanaRandomnessConsumer as Program<SolanaRandomnessConsumer>;
  console.log(`randomnessConsumer: ${consumerProgram.programId}`);

  let switchboard: SwitchboardProgram;
  let switchboardService: FunctionServiceAccount;
  let switchboardFunction: FunctionAccount;
  // let sbNetwork: BootstrappedAttestationQueue;

  before(async () => {
    [switchboard, switchboardFunction, switchboardService] =
      await loadSwitchboard(provider);
  });

  describe("solana-randomness-service", () => {
    it("Is initialized!", async () => {
      const tx = await randomnessService.methods
        .initialize(new BN(10_000))
        .accounts({
          state: programStatePubkey,
          wallet: anchor.utils.token.associatedAddress({
            mint: nativeMint,
            owner: programStatePubkey,
          }),
          mint: nativeMint,
          switchboardFunction: switchboardFunction.publicKey,
          switchboardService: switchboardService.publicKey,
        })
        .rpc();
      console.log("[TX] initialize", tx);
    });

    it("Requests randomness", async () => {
      const request = anchor.web3.Keypair.generate();

      const tx = await randomnessService.methods
        .simpleRandomnessV1(8, {
          programId: randomnessService.programId,
          accounts: [
            {
              pubkey: programStatePubkey,
              isSigner: true,
              isWritable: false,
            },
          ],
          ixData: Buffer.from([]),
        })
        .accounts({
          request: request.publicKey,
          escrow: anchor.utils.token.associatedAddress({
            mint: nativeMint,
            owner: request.publicKey,
          }),
          mint: nativeMint,
          state: programStatePubkey,
        })
        .signers([request])
        .rpc({ skipPreflight: true })
        .catch((e) => {
          console.error(e);
          throw e;
        });

      console.log("[TX] request", tx);

      // await printLogs(randomnessService.provider.connection, tx);

      const requestState =
        await randomnessService.account.simpleRandomnessV1Account.fetch(
          request.publicKey
        );

      console.log(requestState);
    });
  });

  describe("solana-randomness-consumer", () => {
    const [randomnessServiceEventAuthority] =
      anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("__event_authority")],
        randomnessService.programId // uses randomness service pid
      );

    const request = anchor.web3.Keypair.generate();

    it("cpi requests randomness", async () => {
      const config = { commitment: "confirmed" } as const;

      const [event, slot] = await runAndAwaitEvent(
        randomnessService,
        "SimpleRandomnessV1RequestedEvent",
        consumerProgram.methods
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
          .rpc(config)
      );

      assert.strictEqual(
        (event.request as anchor.web3.PublicKey).toBase58(),
        request.publicKey.toBase58()
      );
    });

    it("consumes randomness and invokes callback", async () => {
      const config = { commitment: "confirmed" } as const;

      const result = Buffer.from(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]));

      const requestState =
        await randomnessService.account.simpleRandomnessV1Account.fetch(
          request.publicKey
        );

      const [sbServiceAccount, sbServiceState] =
        await FunctionServiceAccount.load(
          switchboard,
          switchboardService.publicKey
        );
      console.log(
        "enclaveSigner",
        sbServiceState.enclave.enclaveSigner.toBase58()
      );

      const remainingAccounts: anchor.web3.AccountMeta[] = [];

      for (const account of requestState.callback.accounts) {
        if (account.pubkey.equals(request.publicKey)) {
          continue;
        }
        if (account.pubkey.equals(programStatePubkey)) {
          continue;
        }
        if (Boolean(account.isSigner)) {
          console.log(
            {
              pubkey: account.pubkey.toBase58(),
              isSigner: Boolean(account.isSigner),
              isWritable: Boolean(account.isWritable),
            }
          )
        }
        remainingAccounts.push({
          pubkey: account.pubkey,
          isSigner: Boolean(account.isSigner),
          isWritable: Boolean(account.isWritable),
        });
      }

      const signature = await randomnessService.methods
        .simpleRandomnessV1Settle(result)
        .accounts({
          user: requestState.user,
          request: request.publicKey,
          escrow: anchor.utils.token.associatedAddress({
            mint: nativeMint,
            owner: request.publicKey,
          }),
          state: programStatePubkey,
          wallet: anchor.utils.token.associatedAddress({
            mint: nativeMint,
            owner: programStatePubkey,
          }),
          callbackPid: requestState.callback.programId,

          switchboardFunction: switchboardFunction.publicKey,
          switchboardService: switchboardService.publicKey,
          enclaveSigner: sbServiceState.enclave.enclaveSigner,

          instructionsSysvar: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
        })
        .remainingAccounts(remainingAccounts)
        .rpc(config)
        .catch((e) => {
          console.error(e);
          throw e;
        });

      // const signature = await randomnessService.provider
      //   .sendAndConfirm(tx, undefined, { ...config, skipPreflight: true })
      //   .catch((e) => {
      //     console.error(e);
      //     throw e;
      //   });

      console.log("[TX] settle", signature);

      await printLogs(
        randomnessService.provider.connection,
        signature,
        false,
        0
      );
    });
  });
});
