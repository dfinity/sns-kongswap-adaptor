use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager},
    VectorMemory,
};
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use kongswap_adaptor::agent::Request;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, Balances, DepositRequest, TreasuryManager, TreasuryManagerInit,
};
use std::{cell::RefCell, collections::VecDeque, error::Error, fmt::Display};

use crate::{
    state::storage::ConfigState,
    validation::{ValidatedAsset, ValidatedTreasuryManagerInit},
    StableAuditTrail, StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};

use super::*;

const E8: u64 = 10_000_000;

#[derive(Clone, Debug)]
pub struct MockError {
    pub message: String,
}

impl From<String> for MockError {
    fn from(message: String) -> Self {
        MockError { message }
    }
}

impl From<&str> for MockError {
    fn from(message: &str) -> Self {
        MockError {
            message: message.to_string(),
        }
    }
}

impl Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for MockError {}

struct Call {
    raw_request: Vec<u8>,
    raw_reply: Option<Vec<u8>>,
    failure_message: Option<String>,
}

impl Call {
    fn new<Req>(
        request: Req,
        reply: Option<Req::Response>,
        failure_message: Option<String>,
    ) -> Result<Self, ()>
    where
        Req: Request + CandidType,
    {
        // If we define a failure message, we cannot have a reply
        if reply.is_some() && failure_message.is_some() {
            return Err(());
        }

        let raw_request = candid::encode_one(request).map_err(|_| {})?;
        let raw_reply = match reply {
            Some(reply) => {
                let encoded_reply = candid::encode_one(reply);
                match encoded_reply {
                    Ok(reply) => Some(reply),
                    Err(_) => {
                        return Err(());
                    }
                }
            }
            None => None,
        };
        Ok(Self {
            raw_request,
            raw_reply,
            failure_message,
        })
    }
}

struct MockAgent {
    // Add fields to control mock behavior
    expected_calls: VecDeque<Call>,
}

impl MockAgent {
    fn new() -> Self {
        Self {
            expected_calls: VecDeque::default(),
        }
    }

    fn add_call<Req>(
        &mut self,
        request: Req,
        reply: Option<Req::Response>,
        failure_message: Option<String>,
    ) -> Result<(), ()>
    where
        Req: Request + CandidType,
    {
        let call = Call::new(request, reply, failure_message)?;
        self.expected_calls.push_back(call);
        Ok(())
    }
}

impl AbstractAgent for MockAgent {
    type Error = MockError;

    async fn call<R: kongswap_adaptor::agent::Request>(
        &self,
        _canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> Result<R::Response, Self::Error>
    where
        R: CandidType,
    {
        if self.expected_calls.is_empty() {
            return Err(MockError {
                message: "Didn't expect any calls".to_string(),
            });
        }
        // Unwrapping is safe, we have ensured previously
        // that there exists at least one call.
        let expected_call = self.expected_calls.pop_front().unwrap();
        let Ok(raw_request) = candid::encode_one(request) else {
            return Err(MockError {
                message: "Cannot encode the request".to_string(),
            });
        };

        if raw_request != expected_call.raw_request {
            return Err(MockError {
                message: "Request doesn't match the expected one".to_string(),
            });
        }

        if let Some(failure_message) = expected_call.failure_message {
            return Err(MockError {
                message: expected_call.failure_message.unwrap(),
            });
        }

        if let Some(raw_reply) = expected_call.raw_reply {
            return Ok(candid::decode_one::<R::Response>(raw_reply).map_err(|e| e.to_string())?);
        }
    }
}

#[tokio::test]
async fn test_deposit_success() {
    // Create test assets and request first
    let asset_0 = Asset::Token {
        ledger_canister_id: Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap(),
        symbol: "DAO".to_string(),
        ledger_fee_decimals: Nat::from(10_000u64),
    };

    let asset_1 = Asset::Token {
        ledger_canister_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
        symbol: "ICP".to_string(),
        ledger_fee_decimals: Nat::from(10_000u64),
    };

    let owner_account = sns_treasury_manager::Account {
        owner: Principal::from_text("2vxsx-fae").unwrap(),
        subaccount: None,
    };

    // Set up mock agent with expected responses
    let mock_agent =
        MockAgent::new().with_raw_reply(candid::encode_one(Nat::from(1000 * E8)).unwrap()); // Mock allowance response

    // Use VectorMemory for testing - much cleaner

    thread_local! {
        static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
            RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

        static BALANCES: RefCell<StableBalances> =
            MEMORY_MANAGER.with(|memory_manager|
                RefCell::new(
                    StableCell::init(
                        memory_manager.borrow().get(BALANCES_MEMORY_ID),
                        ConfigState::default()
                    )
                    .expect("BALANCES init should not cause errors")
                )
            );

        static AUDIT_TRAIL: RefCell<StableAuditTrail> =
            MEMORY_MANAGER.with(|memory_manager|
                RefCell::new(
                    StableVec::init(
                        memory_manager.borrow().get(AUDIT_TRAIL_MEMORY_ID)
                    )
                    .expect("AUDIT_TRAIL init should not cause errors")
                )
            );
    }

    let canister_id = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let mut kong_adaptor = KongSwapAdaptor::new(
        || 0, // Mock time function
        mock_agent,
        canister_id,
        &BALANCES,
        &AUDIT_TRAIL,
    );

    let allowances = vec![
        Allowance {
            asset: asset_0,
            owner_account,
            amount_decimals: Nat::from(500 * E8),
        },
        Allowance {
            asset: asset_1,
            owner_account,
            amount_decimals: Nat::from(400 * E8),
        },
    ];

    let init = TreasuryManagerInit {
        allowances: allowances.clone(),
    };

    let ValidatedTreasuryManagerInit {
        allowance_0,
        allowance_1,
    } = init.try_into().unwrap();

    // Initialize and test
    kong_adaptor.initialize(
        allowance_0.asset,
        allowance_1.asset,
        allowance_0.owner_account,
        allowance_1.owner_account,
    );

    // This should now work without panicking
    let request = DepositRequest { allowances };
    let result = kong_adaptor.deposit(request).await;

    assert_eq!(result, Ok(Balances::default()),);
}
