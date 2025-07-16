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

struct CallSpec {
    raw_request: Vec<u8>,
    raw_reply: Option<Vec<u8>>,
    failure_message: Option<String>,
}

impl CallSpec {
    fn new<Req>(
        request: Req,
        result: Result<Option<Req::Response>, Option<String>>,
    ) -> Result<Self, ()>
    where
        Req: Request + CandidType,
    {
        let raw_request = candid::encode_one(request).expect("Request is not encodable");

        let (raw_reply, failure_message) = match result {
            Ok(response) => match response {
                Some(reply) => {
                    let encoded_reply =
                        candid::encode_one(reply).expect("Response is not encodable");
                    (Some(encoded_reply), None)
                }
                None => (None, None),
            },
            Err(failure_message) => (None, failure_message),
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
    expected_calls: VecDeque<CallSpec>,
}

impl MockAgent {
    fn new() -> Self {
        Self {
            expected_calls: VecDeque::default(),
        }
    }

    fn add_call<Req>(
        mut self,
        request: Req,
        result: Result<Option<Req::Response>, Option<String>>,
    ) -> Self
    where
        Req: Request + CandidType,
    {
        let call =
            CallSpec::new(request, result).expect("Creating a new call specification failed");
        self.expected_calls.push_back(call);
        self
    }

    fn finished_calls(&self) -> bool {
        self.expected_calls.is_empty()
    }
}

impl AbstractAgent for MockAgent {
    type Error = MockError;

    async fn call<R: kongswap_adaptor::agent::Request>(
        &mut self,
        _canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> Result<R::Response, Self::Error>
    where
        R: CandidType,
    {
        let expected_call = self
            .expected_calls
            .pop_front()
            .expect("Consumed all expected requests");

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
                message: failure_message,
            });
        }

        if let Some(raw_reply) = expected_call.raw_reply {
            let reply = candid::decode_one::<R::Response>(&raw_reply)
                .expect("Unable to decode the response");
            return Ok(reply);
        }

        return Err(MockError {
            message: "No replies found for the request".to_string(),
        });
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

    let request = DepositRequest {
        allowances: allowances.clone(),
    };
    let result = Ok(Some(Ok(Balances::default())));
    // Set up mock agent with expected responses
    let mock_agent = MockAgent::new().add_call(request.clone(), result);

    let canister_id = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let mut kong_adaptor = KongSwapAdaptor::new(
        || 0, // Mock time function
        mock_agent,
        canister_id,
        &BALANCES,
        &AUDIT_TRAIL,
    );

    let init = TreasuryManagerInit { allowances };

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
    let result = kong_adaptor.deposit(request).await;

    assert_eq!(result, Ok(Balances::default()),);
}
