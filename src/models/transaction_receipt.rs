use reth_primitives::bloom::logs_bloom;
use reth_primitives::contract::create_address;
use reth_primitives::{U128, U256, U64, U8};
use reth_rpc_types::{Log, TransactionReceipt as EthTransactionReceipt};
use starknet::core::types::{
    ExecutionResult, InvokeTransactionReceipt, MaybePendingTransactionReceipt, TransactionReceipt,
};
use starknet::providers::Provider;
use tracing::debug;

use super::event::StarknetEvent;
use super::felt::Felt252Wrapper;
use super::transaction::transaction::StarknetTransaction;
use crate::into_via_wrapper;
use crate::starknet_client::errors::EthApiError;
use crate::starknet_client::KakarotClient;

pub struct StarknetTransactionReceipt(MaybePendingTransactionReceipt);

impl From<MaybePendingTransactionReceipt> for StarknetTransactionReceipt {
    fn from(receipt: MaybePendingTransactionReceipt) -> Self {
        Self(receipt)
    }
}

impl From<StarknetTransactionReceipt> for MaybePendingTransactionReceipt {
    fn from(receipt: StarknetTransactionReceipt) -> Self {
        receipt.0
    }
}

impl StarknetTransactionReceipt {
    #[tracing::instrument(skip_all, level = "debug")]
    pub async fn to_eth_transaction_receipt<P: Provider + Send + Sync>(
        self,
        client: &KakarotClient<P>,
    ) -> Result<Option<EthTransactionReceipt>, EthApiError> {
        let starknet_tx_receipt: MaybePendingTransactionReceipt = self.into();

        debug!("starknet transaction receipt: {:?}", starknet_tx_receipt);

        let res_receipt = match starknet_tx_receipt {
            MaybePendingTransactionReceipt::Receipt(receipt) => match receipt {
                TransactionReceipt::Invoke(InvokeTransactionReceipt {
                    transaction_hash,
                    execution_result,
                    block_hash,
                    block_number,
                    events,
                    ..
                }) => {
                    let starknet_tx: StarknetTransaction =
                        client.starknet_provider().get_transaction_by_hash(transaction_hash).await?.into();

                    let transaction_hash = Some(into_via_wrapper!(transaction_hash));
                    let block_hash = Some(into_via_wrapper!(block_hash));
                    let block_number = Some(into_via_wrapper!(block_number));

                    let eth_tx = starknet_tx.to_eth_transaction(client, block_hash, block_number, None).await?;
                    let from = eth_tx.from;
                    let to = eth_tx.to;
                    let contract_address = match execution_result {
                        ExecutionResult::Succeeded => {
                            match to {
                                // If to is Some, means contract_address should be None as it is a normal transaction
                                Some(_) => None,
                                // If to is None, is a contract creation transaction so contract_address should be Some
                                None => Some(create_address(eth_tx.from, eth_tx.nonce.as_u64())),
                            }
                        }
                        ExecutionResult::Reverted { ref reason } => {
                            tracing::error!("Transaction reverted with {reason}");
                            None
                        }
                    };

                    let status_code = match execution_result {
                        ExecutionResult::Succeeded => Some(U64::from(1)),
                        ExecutionResult::Reverted { .. } => Some(U64::from(0)),
                    };

                    let logs: Vec<Log> = events
                        .into_iter()
                        .map(StarknetEvent::new)
                        .filter_map(|event| {
                            event.to_eth_log(client, block_hash, block_number, transaction_hash, None, None).ok()
                        })
                        .collect();

                    // Reth note:
                    // This bloom operation is slow and should be cached if possible.
                    let bloom = {
                        let logs: Vec<reth_primitives::Log> = logs
                            .iter()
                            .map(|log| reth_primitives::Log {
                                data: log.data.clone(),
                                topics: log.topics.clone(),
                                address: log.address,
                            })
                            .collect();
                        logs_bloom(logs.iter())
                    };

                    EthTransactionReceipt {
                        transaction_hash,
                        transaction_index: U64::from(0), // TODO: Fetch real data
                        block_hash,
                        block_number,
                        from,
                        to,
                        cumulative_gas_used: U256::from(1_000_000), // TODO: Fetch real data
                        gas_used: Some(U256::from(500_000)),
                        contract_address,
                        logs,
                        state_root: None,
                        logs_bloom: bloom,
                        status_code,
                        effective_gas_price: U128::from(1_000_000), // TODO: Fetch real data
                        transaction_type: U8::from(0),              // TODO: Fetch real data
                        blob_gas_price: None,
                        blob_gas_used: None,
                    }
                }
                // L1Handler, Declare, Deploy and DeployAccount transactions unsupported for now in
                // Kakarot
                _ => return Ok(None),
            },
            MaybePendingTransactionReceipt::PendingReceipt(_) => {
                return Ok(None);
            }
        };

        debug!("ethereum transaction receipt: {:?}", res_receipt);

        Ok(Some(res_receipt))
    }
}
