use std::{cell::RefCell, error::Error, fmt::Display};
use candid::Principal;
use ic_stable_structures::{memory_manager::{MemoryId, MemoryManager}, VectorMemory};
use sns_treasury_manager::{Allowance, Asset, Balances, DepositRequest, TreasuryManager, TreasuryManagerInit};
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use pretty_assertions::assert_eq;

use crate::{state::storage::ConfigState, validation::{ValidatedAsset, ValidatedTreasuryManagerInit}, StableAuditTrail, StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID};

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

struct MockAgent {
    // Add fields to control mock behavior
    should_fail: bool,
    error_message: Option<MockError>,
    raw_reply: Option<Vec<u8>>,
}

impl MockAgent {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            error_message: None,
            raw_reply: None,
        }
    }
    
    pub fn with_error(mut self, error: &str) -> Self {
        self.should_fail = true;
        self.error_message = Some(error.into());
        self
    }

    pub fn with_raw_reply(mut self, blob: Vec<u8>) -> Self {
        self.raw_reply = Some(blob);
        self
    }
}

impl AbstractAgent for MockAgent {
    type Error = MockError;

    async fn call<R: kongswap_adaptor::agent::Request>(
        &self,
        _canister_id: impl Into<Principal> + Send,
        _request: R,
    ) -> Result<R::Response, Self::Error> {
        if self.should_fail {
            return Err(self.error_message.clone().unwrap_or_else(|| "Mock error".into()));
        }

        if let Some(raw_reply) = &self.raw_reply {
            return Ok(candid::decode_one::<R::Response>(raw_reply).map_err(|e| e.to_string())?);
        }
        
        // Return mock responses based on request type
        // You'll need to implement specific mock responses for each request type
        todo!("The test should specify the expected error or reply for each MockAgent call.")
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
    let mock_agent = MockAgent::new()
        .with_raw_reply(candid::encode_one(Nat::from(1000 * E8)).unwrap()); // Mock allowance response
    
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

    let init = TreasuryManagerInit { allowances: allowances.clone() };

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
    let result = kong_adaptor.deposit(request).await.unwrap();

    assert_eq!(
        result,
        Balances::default(),
    );
}
