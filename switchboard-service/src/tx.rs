/// This module contains the logic for handling transaction and simulation errors to
/// determine if a transaction should be retried.
use crate::*;

use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_program::instruction::InstructionError;
use solana_sdk::transaction::TransactionError;
use switchboard_solana::rust_decimal::prelude::Zero;

const REQUEST_ACCOUNT_DOESNT_EXIST_ERROR_STR: &str = "Program log: AnchorError caused by account: request. Error Code: AccountOwnedByWrongProgram. Error Number: 3007. Error Message: The given account is owned by a different program than expected.\nProgram log: Left:\nProgram log: 11111111111111111111111111111111";

pub async fn is_tx_error_retryable(err: &SbError) -> bool {
    match err {
        SbError::CustomError { message, source: e } => {
            if message.starts_with("Failed to broadcast transaction") {
                if let Some(client_err) = e.downcast_ref::<ClientError>() {
                    match client_err.kind() {
                        ClientErrorKind::RpcError(rpc_err) => {
                            error!("Randomness Rpc Error: {:#?}", rpc_err);

                            // Extract the rpc response error
                            if let RpcError::RpcResponseError {
                                code,
                                message,
                                data,
                            } = rpc_err
                            {
                                error!("[{}] {} - Rpc Response Error: {:#?}", code, message, data);
                                if let RpcResponseErrorData::SendTransactionPreflightFailure(
                                    RpcSimulateTransactionResult {
                                        err: Some::<TransactionError>(tx_err),
                                        logs,
                                        accounts,
                                        units_consumed,
                                        return_data,
                                    },
                                ) = data
                                {
                                    let logs = logs.clone().unwrap_or_default().join("\n");
                                    error!("Simulation Error: {:#?}", tx_err);

                                    // @DEV - here we can handle the tx_error
                                    match tx_err {
                                        TransactionError::BlockhashNotFound => {
                                            error!("BlockhashNotFound: {:#?}", tx_err);
                                            // TODO: retry
                                        },
                                        TransactionError::InstructionError(idx, ix_error) => {
                                            // @DEV - here we assume ixn 0 is the randomness ixn. if we add priority fees or compute units this will change.
                                            if idx.is_zero() {
                                                error!("Randomness Instruction Error: {:#?}", ix_error);
                                                match ix_error {
                                                    InstructionError::Custom(code) => {
                                                        match code {
                                                            // AccountOwnedByWrongProgram - no action needed
                                                            3007 => {
                                                                if logs.contains(&REQUEST_ACCOUNT_DOESNT_EXIST_ERROR_STR) {
                                                                    // no action needed
                                                                    info!("Randomness Request Already Settled");
                                                                    return false;
                                                                } else {
                                                                    error!("[3007] Randomness Instruction Error: {:#?}\n{}", ix_error, logs);
                                                                }
                                                            }
                                                            // handle other cases here
                                                            _ => {
                                                                error!("[{}] Randomness Instruction Error: {:#?}\n{}", code, ix_error, logs);
                                                            }
                                                        }
                                                        // @DEV - here we assume that the only custom error is that the request has already been settled.
                                                        // @TODO - we should handle this better
                                                        error!("Randomness Request Already Settled");
                                                    }
                                                    _ => {
                                                        error!("Randomness Instruction Error: {:#?}\n{}", ix_error, logs);
                                                    }
                                                }
                                            } else {
                                                error!("Instruction Error: {:#?}\n{}", ix_error, logs);
                                            }
                                        },

                                        _ => {
                                            error!("Simulation Tx Error: {:#?}\n{}", tx_err, logs);
                                        }

                                        // TransactionError::AccountInUse => todo!(),
                                        // TransactionError::AccountLoadedTwice => todo!(),
                                        // TransactionError::AccountNotFound => todo!(),
                                        // TransactionError::ProgramAccountNotFound => todo!(),
                                        // TransactionError::InsufficientFundsForFee => todo!(),
                                        // TransactionError::InvalidAccountForFee => todo!(),
                                        // TransactionError::AlreadyProcessed => todo!(),
                                        // TransactionError::CallChainTooDeep => todo!(),
                                        // TransactionError::MissingSignatureForFee => todo!(),
                                        // TransactionError::InvalidAccountIndex => todo!(),
                                        // TransactionError::SignatureFailure => todo!(),
                                        // TransactionError::InvalidProgramForExecution => todo!(),
                                        // TransactionError::SanitizeFailure => todo!(),
                                        // TransactionError::ClusterMaintenance => todo!(),
                                        // TransactionError::AccountBorrowOutstanding => todo!(),
                                        // TransactionError::WouldExceedMaxBlockCostLimit => todo!(),
                                        // TransactionError::UnsupportedVersion => todo!(),
                                        // TransactionError::InvalidWritableAccount => todo!(),
                                        // TransactionError::WouldExceedMaxAccountCostLimit => todo!(),
                                        // TransactionError::WouldExceedAccountDataBlockLimit => todo!(),
                                        // TransactionError::TooManyAccountLocks => todo!(),
                                        // TransactionError::AddressLookupTableNotFound => todo!(),
                                        // TransactionError::InvalidAddressLookupTableOwner => todo!(),
                                        // TransactionError::InvalidAddressLookupTableData => todo!(),
                                        // TransactionError::InvalidAddressLookupTableIndex => todo!(),
                                        // TransactionError::InvalidRentPayingAccount => todo!(),
                                        // TransactionError::WouldExceedMaxVoteCostLimit => todo!(),
                                        // TransactionError::WouldExceedAccountDataTotalLimit => todo!(),
                                        // TransactionError::DuplicateInstruction(_) => todo!(),
                                        // TransactionError::InsufficientFundsForRent { account_index } => todo!(),
                                        // TransactionError::MaxLoadedAccountsDataSizeExceeded => todo!(),
                                        // TransactionError::InvalidLoadedAccountsDataSizeLimit => todo!(),
                                        // TransactionError::ResanitizationNeeded => todo!(),
                                        // TransactionError::ProgramExecutionTemporarilyRestricted { account_index } => todo!(),
                                        // TransactionError::UnbalancedTransaction => todo!(),
                                    }
                                }
                            }
                        }
                        ClientErrorKind::TransactionError(tx_err) => {
                            error!("Randomness Tx Error: {:#?}", tx_err);
                        }
                        _ => {
                            error!("Failed to broadcast transaction: {:#?}", client_err);
                        }
                    }
                } else {
                    error!("Failed to broadcast transaction: {:#?}", e);
                }
            } else {
                error!("Randomness Txn Error: {:#?}", e);
            }
        }
        _ => {
            error!("Randomness Txn Error: {:#?}", err);
            // TODO: should we retry?
        }
    }

    true
}
