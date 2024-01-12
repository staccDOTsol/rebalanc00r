import {
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
import type { SwitchboardProgram } from "@switchboard-xyz/solana.js";
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
  let switchboardService: anchor.web3.PublicKey;
  let switchboardFunction: anchor.web3.PublicKey;

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
          switchboardFunction: switchboardFunction,
          switchboardService: switchboardService,
        })
        .rpc();
      console.log("[TX] initialize", tx);
    });

    it("Requests randomness", async () => {
      const request = anchor.web3.Keypair.generate();

      const tx = await randomnessService.methods
        .request(8, {
          programId: anchor.web3.PublicKey.default,
          accounts: [],
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

      // const requestState =
      //   await randomnessService.account.randomnessRequest.fetch(
      //     request.publicKey
      //   );

      // console.log(requestState);
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
        "RandomnessRequestedEvent",
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

    // it("consumes randomness and invokes callback", async () => {
    //   const config = { commitment: "confirmed" } as const;

    //   const result = Buffer.from(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]));

    //   const requestState =
    //     await randomnessService.account.randomnessRequest.fetch(
    //       request.publicKey
    //     );

    //   console.log(`Request: ${request.publicKey}`);

    //   const settleIx = await randomnessService.methods
    //     .settle(result)
    //     .accounts({
    //       request: request.publicKey,
    //       escrow: anchor.utils.token.associatedAddress({
    //         mint: nativeMint,
    //         owner: request.publicKey,
    //       }),
    //       state: programStatePubkey,
    //       wallet: anchor.utils.token.associatedAddress({
    //         mint: nativeMint,
    //         owner: programStatePubkey,
    //       }),
    //       callbackPid: requestState.callback.programId,

    //       switchboardFunction: SWITCHBOARD_FUNCTION_PUBKEY,
    //       switchboardService: SWITCHBOARD_SERVICE_PUBKEY,
    //     })
    //     .instruction()
    //     .catch((e) => {
    //       console.error(e);
    //       throw e;
    //     });

    //   for (const account of requestState.callback.accounts) {
    //     if (!account.pubkey.equals(request.publicKey)) {
    //       continue;
    //     }
    //     if (!account.pubkey.equals(programStatePubkey)) {
    //       continue;
    //     }
    //     settleIx.keys.push({
    //       pubkey: account.pubkey,
    //       isSigner: Boolean(account.isSigner),
    //       isWritable: Boolean(account.isWritable),
    //     });
    //   }

    //   const tx = new anchor.web3.Transaction({
    //     ...(await randomnessService.provider.connection.getLatestBlockhash()),
    //   });
    //   tx.add(settleIx);

    //   const signature = await randomnessService.provider
    //     .sendAndConfirm(tx, undefined, { ...config, skipPreflight: true })
    //     .catch((e) => {
    //       console.error(e);
    //       throw e;
    //     });

    //   console.log("[TX] settle", signature);

    //   await printLogs(
    //     randomnessService.provider.connection,
    //     signature,
    //     false,
    //     0
    //   );
    // });
  });
});
