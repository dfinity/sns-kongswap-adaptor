use candid::{Decode, Encode, Principal};
use ic_stable_structures::{storable::Bound, Storable};
use sns_treasury_manager::{
    Transaction, TransactionError, TransactionWitness, TreasuryManagerOperation,
};
use std::borrow::Cow;

use crate::validation::ValidatedBalances;

#[derive(candid::CandidType, candid::Deserialize, Clone, Debug)]
pub(crate) struct StableTransaction {
    pub timestamp_ns: u64,
    pub canister_id: Principal,
    pub result: Result<TransactionWitness, TransactionError>,
    pub human_readable: String,
    pub treasury_manager_operation: TreasuryManagerOperation,
}

impl Storable for StableTransaction {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 2048, // Increased size to accommodate all fields
        is_fixed_size: false,
    };
}

impl Storable for ValidatedBalances {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 410,
        is_fixed_size: true,
    };
}

impl From<StableTransaction> for Transaction {
    fn from(item: StableTransaction) -> Self {
        Self {
            timestamp_ns: item.timestamp_ns,
            canister_id: item.canister_id,
            result: item.result,
            human_readable: item.human_readable,
            treasury_manager_operation: item.treasury_manager_operation,
        }
    }
}

impl From<Transaction> for StableTransaction {
    fn from(item: Transaction) -> Self {
        Self {
            timestamp_ns: item.timestamp_ns,
            canister_id: item.canister_id,
            result: item.result,
            human_readable: item.human_readable,
            treasury_manager_operation: item.treasury_manager_operation,
        }
    }
}
