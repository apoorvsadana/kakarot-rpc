use reth_primitives::{Address, Bytes, H256, U256};
use reth_rpc_types::Log;
use starknet::core::types::Event;
use starknet::providers::Provider;

use super::felt::Felt252Wrapper;
use crate::starknet_client::errors::EthApiError;
use crate::starknet_client::helpers::try_from_u8_iterator;
use crate::starknet_client::KakarotClient;
use crate::{into_via_wrapper, try_into_via_wrapper};

#[derive(Debug, Clone)]
pub struct StarknetEvent(Event);

impl StarknetEvent {
    pub const fn new(sn_event: Event) -> Self {
        Self(sn_event)
    }
}

impl From<Event> for StarknetEvent {
    fn from(event: Event) -> Self {
        Self::new(event)
    }
}

impl StarknetEvent {
    pub fn to_eth_log<P: Provider + Send + Sync>(
        self,
        client: &KakarotClient<P>,
        block_hash: Option<H256>,
        block_number: Option<U256>,
        transaction_hash: Option<H256>,
        log_index: Option<U256>,
        transaction_index: Option<U256>,
    ) -> Result<Log, EthApiError> {
        // If event `from_address` does not equal kakarot address, return early
        if self.0.from_address != client.kakarot_address() {
            return Err(EthApiError::KakarotDataFilteringError("Event".into()));
        }

        // Derive the evm address from the first item in the `event.keys` vector and remove it
        let (evm_contract_address, keys) =
            self.0.keys.split_first().ok_or_else(|| EthApiError::KakarotDataFilteringError("Event".into()))?;

        let address: Address = try_into_via_wrapper!(*evm_contract_address);

        if keys.len() % 2 != 0 {
            return Err(anyhow::anyhow!("Not a convertible event: Keys length is not even").into());
        }

        let topics: Vec<H256> = keys
            .chunks(2)
            .map(|chunk| {
                let low: U256 = into_via_wrapper!(chunk[0]);
                let high: U256 = into_via_wrapper!(chunk[1]);
                let val = low | (high << 128);
                H256::from(val)
            })
            .collect();

        let data = Bytes::from(try_from_u8_iterator::<_, Vec<_>>(self.0.data.into_iter()));

        Ok(Log {
            address,
            topics,
            data,
            block_hash,
            block_number,
            transaction_hash,
            log_index,
            transaction_index,
            removed: false,
        })
    }
}
