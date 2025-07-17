use candid::{CandidType, Principal};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue;
use kongswap_adaptor::agent::icrc_requests::Icrc1MetadataRequest;
use kongswap_adaptor::{agent::Request, requests::CommitStateRequest};
use pretty_assertions::assert_eq;
use serde::de::DeserializeOwned;
use sns_treasury_manager::{
    Allowance, Asset, Balances, DepositRequest, TreasuryManager, TreasuryManagerInit,
};
use std::{cell::RefCell, collections::VecDeque, error::Error, fmt::Display};

use super::*;
use crate::kong_types::{
    AddPoolReply, AddTokenArgs, AddTokenReply, ICReply, RemoveLiquidityAmountsArgs,
    RemoveLiquidityAmountsReply, UpdateTokenArgs, UpdateTokenReply, UserBalanceLPReply,
    UserBalancesArgs, UserBalancesReply,
};
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use std::fmt::Debug;

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

// TODO use Result to store reply and failure
struct CallSpec {
    raw_request: Vec<u8>,
    raw_response: Vec<u8>,
    canister_id: Principal,
}

impl CallSpec {
    fn new<Req>(canister_id: Principal, request: Req, response: Req::Response) -> Result<Self, ()>
    where
        Req: Request,
    {
        let raw_request = request.payload().expect("Request is not encodable");
        let raw_response = candid::encode_one(response).expect("Response is not encodable");

        Ok(Self {
            raw_request,
            raw_response,
            canister_id,
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
        canister_id: Principal,
        request: Req,
        response: Req::Response,
    ) -> Self
    where
        Req: Request,
    {
        let call = CallSpec::new(canister_id, request, response)
            .expect("Creating a new call specification failed");
        self.expected_calls.push_back(call);
        let commit_state = CallSpec::new(*KONG_BACKEND_CANISTER_ID, CommitStateRequest {}, ())
            .expect("CommittState call creation failed");
        self.expected_calls.push_back(commit_state);
        self
    }

    fn finished_calls(&self) -> bool {
        self.expected_calls.is_empty()
    }
}

impl AbstractAgent for MockAgent {
    type Error = MockError;
    // Infallable !
    async fn call<R: kongswap_adaptor::agent::Request + Debug + CandidType>(
        &mut self,
        canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> Result<R::Response, Self::Error> {
        println!("started call...");
        let Ok(raw_request) = request.payload() else {
            panic!("Cannot encode the request");
        };

        let expected_call = self
            .expected_calls
            .pop_front()
            .expect("Consumed all expected requests");

        if raw_request != expected_call.raw_request {
            println!("request: {:#?}", request);
            println!("{:?}\n{:?}", raw_request, expected_call.raw_request);
            panic!("Request doesn't match");
        }
        let canister_id = canister_id.into();

        if canister_id != expected_call.canister_id {
            println!("request canister id: {}", canister_id);
            panic!("Canister IDs doesn't match");
        }

        let reply = candid::decode_one::<R::Response>(&expected_call.raw_response)
            .expect("Unable to decode the response");

        println!("successfully called canister ID: {}", canister_id);
        return Ok(reply);
    }
}

fn make_approve_request(amount: u64, fee: u64) -> ApproveArgs {
    ApproveArgs {
        from_subaccount: None,
        spender: Account {
            owner: *KONG_BACKEND_CANISTER_ID,
            subaccount: None,
        },
        // All approved tokens should be fully used up before the next deposit.
        amount: Nat::from(amount - fee),
        expected_allowance: Some(Nat::from(0u8)),
        expires_at: Some(u64::MAX),
        memo: None,
        created_at_time: None,
        fee: Some(fee.into()),
    }
}

fn make_balance_request(self_id: Principal) -> Account {
    Account {
        owner: self_id,
        subaccount: None,
    }
}

fn make_add_token_request(token: String) -> AddTokenArgs {
    AddTokenArgs { token }
}

fn make_add_token_reply(
    token_id: u32,
    chain: String,
    canister_id: Principal,
    name: String,
    symbol: String,
    fee: u64,
) -> AddTokenReply {
    AddTokenReply::IC(ICReply {
        token_id,
        chain,
        canister_id: canister_id.to_string(),
        name,
        symbol,
        decimals: 8,
        fee: Nat::from(fee),
        icrc1: true,
        icrc2: true,
        icrc3: true,
        is_removed: false,
    })
}

fn make_update_token_request(token: String) -> UpdateTokenArgs {
    UpdateTokenArgs { token }
}

fn make_update_token_reply(
    token_id: u32,
    chain: String,
    canister_id: Principal,
    name: String,
    symbol: String,
    fee: u64,
) -> UpdateTokenReply {
    UpdateTokenReply::IC(ICReply {
        token_id,
        chain,
        canister_id: canister_id.to_string(),
        name,
        symbol,
        decimals: 8,
        fee: Nat::from(fee),
        icrc1: true,
        icrc2: true,
        icrc3: true,
        is_removed: false,
    })
}

fn make_metadata_reply(name: String, symbol: String, fee: u64) -> Vec<(String, MetadataValue)> {
    vec![
        (
            "icrc1:decimals".to_string(),
            MetadataValue::Nat(Nat::from(8_u64)),
        ),
        ("icrc1:name".to_string(), MetadataValue::Text(name)),
        ("icrc1:symbol".to_string(), MetadataValue::Text(symbol)),
        ("icrc1:fee".to_string(), MetadataValue::Nat(Nat::from(fee))),
        (
            "icrc1:max_memo_length".to_string(),
            MetadataValue::Nat(Nat::from(32_u64)),
        ),
        (
            "icrc103:public_allowances".to_string(),
            MetadataValue::Text("true".to_string()),
        ),
        (
            "icrc103:max_take_value".to_string(),
            MetadataValue::Nat(Nat::from(500_u64)),
        ),
    ]
}

fn make_add_pool_request(
    token_0: String,
    amount_0: u64,
    token_1: String,
    amount_1: u64,
) -> AddPoolArgs {
    AddPoolArgs {
        token_0,
        amount_0: Nat::from(amount_0),
        tx_id_0: None,
        token_1,
        amount_1: Nat::from(amount_1),
        tx_id_1: None,
        lp_fee_bps: Some(30),
    }
}

fn make_user_balances_request(self_id: Principal) -> UserBalancesArgs {
    UserBalancesArgs {
        principal_id: self_id.to_text(),
    }
}

fn make_user_balance_reply() -> UserBalancesReply {
    UserBalancesReply::LP(UserBalanceLPReply {
        symbol: "DAO_ICP".to_string(),
        balance: 100.0,
        ..Default::default()
    })
}

fn make_remove_liquidity_amounts_request(
    token_0: String,
    token_1: String,
    remove_lp_token_amount: u64,
) -> RemoveLiquidityAmountsArgs {
    RemoveLiquidityAmountsArgs {
        token_0,
        token_1,
        remove_lp_token_amount: Nat::from(remove_lp_token_amount),
    }
}

fn make_remove_liquidity_amounts_reply(
    amount_0: u64,
    amount_1: u64,
) -> RemoveLiquidityAmountsReply {
    RemoveLiquidityAmountsReply {
        amount_0: Nat::from(amount_0),
        amount_1: Nat::from(amount_1),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_deposit_success() {
    const FEE_SNS: u64 = 10_500u64;
    const FEE_ICP: u64 = 9_500u64;
    let sns_ledger = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let icp_ledger = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    let sns_id = Principal::from_text("jg2ra-syaaa-aaaaq-aaewa-cai").unwrap();
    let token_0 = format!("IC.{}", sns_ledger);
    let token_1 = format!("IC.{}", icp_ledger);
    // Create test assets and request first
    let asset_0 = Asset::Token {
        ledger_canister_id: sns_ledger,
        symbol: "DAO".to_string(),
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    let asset_1 = Asset::Token {
        ledger_canister_id: icp_ledger,
        symbol: "ICP".to_string(),
        ledger_fee_decimals: Nat::from(FEE_ICP),
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

    let amount_0_decimals = 500 * E8;
    let amount_1_decimals = 400 * E8;
    let allowances = vec![
        // SNS
        Allowance {
            asset: asset_0,
            owner_account,
            amount_decimals: Nat::from(amount_0_decimals),
        },
        // ICP
        Allowance {
            asset: asset_1,
            owner_account,
            amount_decimals: Nat::from(amount_1_decimals),
        },
    ];

    let mock_agent = MockAgent::new()
        .add_call(
            sns_ledger,
            make_approve_request(amount_0_decimals, FEE_SNS),
            Ok(Nat::from(amount_0_decimals)),
        )
        .add_call(
            icp_ledger,
            make_approve_request(amount_1_decimals, FEE_ICP),
            Ok(Nat::from(amount_1_decimals)),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(amount_0_decimals - FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(token_0.clone()),
            Ok(make_add_token_reply(
                1,
                "IC".to_string(),
                sns_id,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(token_1.clone()),
            Ok(make_add_token_reply(
                2,
                "IC".to_string(),
                icp_ledger,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(token_0.clone()),
            Ok(make_update_token_reply(
                1,
                "IC".to_string(),
                sns_id,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
        )
        .add_call(
            sns_ledger,
            Icrc1MetadataRequest {},
            make_metadata_reply("My DAO Token".to_string(), "DAO".to_string(), FEE_SNS),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(token_1.clone()),
            Ok(make_update_token_reply(
                2,
                "IC".to_string(),
                icp_ledger,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
        )
        .add_call(
            icp_ledger,
            Icrc1MetadataRequest {},
            make_metadata_reply("Internet Computer".to_string(), "ICP".to_string(), FEE_ICP),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_pool_request(
                token_0.clone(),
                amount_0_decimals - 2 * FEE_SNS,
                token_1.clone(),
                amount_1_decimals - 2 * FEE_ICP,
            ),
            Ok(AddPoolReply::default()),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*KONG_BACKEND_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(token_0.clone()),
            Ok(make_update_token_reply(
                1,
                "IC".to_string(),
                sns_id,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
        )
        .add_call(
            sns_ledger,
            Icrc1MetadataRequest {},
            make_metadata_reply("My DAO Token".to_string(), "DAO".to_string(), FEE_SNS),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(token_1.clone()),
            Ok(make_update_token_reply(
                2,
                "IC".to_string(),
                icp_ledger,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
        )
        .add_call(
            icp_ledger,
            Icrc1MetadataRequest {},
            make_metadata_reply("Internet Computer".to_string(), "ICP".to_string(), FEE_ICP),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_user_balances_request(*KONG_BACKEND_CANISTER_ID),
            Ok(vec![make_user_balance_reply()]),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_remove_liquidity_amounts_request(
                "DAO".to_string(),
                "ICP".to_string(),
                10000000000,
            ),
            Ok(make_remove_liquidity_amounts_reply(
                amount_0_decimals - FEE_SNS,
                amount_1_decimals - FEE_ICP,
            )),
        );

    let mut kong_adaptor = KongSwapAdaptor::new(
        || 0, // Mock time function
        mock_agent,
        *KONG_BACKEND_CANISTER_ID,
        &BALANCES,
        &AUDIT_TRAIL,
    );

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
    let result = kong_adaptor.deposit(DepositRequest { allowances }).await;

    assert!(
        kong_adaptor.agent.finished_calls(),
        "There are still some calls remaining"
    );

    assert_eq!(result, Ok(Balances::default()),);
}
