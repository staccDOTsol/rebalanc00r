import * as anchor from "@coral-xyz/anchor";
import { SwitchboardProgram } from "@switchboard-xyz/solana.js";

(async () => {
  const sampleLogs = [
    "Program ComputeBudget111111111111111111111111111111 invoke [1]",
    "Program ComputeBudget111111111111111111111111111111 success",
    "Program ComputeBudget111111111111111111111111111111 invoke [1]",
    "Program ComputeBudget111111111111111111111111111111 success",
    "Program SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f invoke [1]",
    "Program log: Instruction: AggregatorSaveResult",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]",
    "Program log: Instruction: Transfer",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4736 of 260493 compute units",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success",
    "Program data: Dk7x7N2nVamWMG/0TOpJQKzuElh2ihk6oOrzrKp29TiDV246RZrxYYE2Xg4AAAAA+BWfZQAAAABwFwAAAAAAAA==",
    "Program data: A5o8/ZicmX6WMG/0TOpJQKzuElh2ihk6oOrzrKp29TiDV246RZrxYdzHrvus0AQAAAAAAAAAAAAQAAAAgTZeDgAAAAD4FZ9lAAAAANXxep5V6gMK8/DA2ZKRPpRP1nxR7Yu9OysDvAhewTlCAAAAAA==",
    "Program log: P1 B7Gzb3BubnEHVtMNYaE1EagkTk9r6MLBnDLkGpWgdW9E",
    "Program log: MODE_SLIDING",
    "Program data: cB8z6WFkK/WWMG/0TOpJQKzuElh2ihk6oOrzrKp29TiDV246RZrxYdzHrvus0AQAAAAAAAAAAAAQAAAAgTZeDgAAAAD4FZ9lAAAAAAEAAADV8XqeVeoDCvPwwNmSkT6UT9Z8Ue2LvTsrA7wIXsE5QgEAAADcx677rNAEAAAAAAAAAAAAEAAAAA==",
    "Program log: Reward: 12500",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]",
    "Program log: Instruction: Transfer",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4736 of 155929 compute units",
    "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success",
    "Program SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f consumed 151464 of 299700 compute units",
    "Program SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f success",
  ];

  const program = await SwitchboardProgram.load(
    new anchor.web3.Connection(anchor.web3.clusterApiUrl("devnet"))
  );

  const eventParser = new anchor.EventParser(
    program.oracleProgramId,
    new anchor.BorshCoder(await program.oracleProgramIdl)
  );

  const events = eventParser.parseLogs(sampleLogs);

  for (const event of eventParser.parseLogs(sampleLogs)) {
    console.log(
      JSON.stringify(
        event,
        (key, value) => {
          if (anchor.BN.isBN(value)) {
            return value.toString();
          }
          return value;
        },
        2
      )
    );
  }
})();
