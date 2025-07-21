use candid::{CandidType, Nat, Principal};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue;
use icrc_ledger_types::icrc2::approve::ApproveArgs;
use kongswap_adaptor::agent::icrc_requests::Icrc1MetadataRequest;
use kongswap_adaptor::{agent::Request, requests::CommitStateRequest};
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, DepositRequest, TreasuryManager, TreasuryManagerInit,
};
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::{cell::RefCell, collections::VecDeque, error::Error, fmt::Display};
use tokio::sync::Barrier;

use super::*;
use crate::kong_types::{
    AddPoolArgs, AddPoolReply, AddTokenArgs, AddTokenReply, ICReply, RemoveLiquidityAmountsArgs,
    RemoveLiquidityAmountsReply, UpdateTokenArgs, UpdateTokenReply, UserBalanceLPReply,
    UserBalancesArgs, UserBalancesReply,
};
use crate::KONG_BACKEND_CANISTER_ID;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use std::fmt::Debug;

const E8: u64 = 100_000_000;
const FEE_SNS: u64 = 10_500u64;
const FEE_ICP: u64 = 9_500u64;

use lazy_static::lazy_static;

pub struct UnsafeSyncCell<T>(pub UnsafeCell<T>);

impl<T> UnsafeSyncCell<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }
}

// SAFETY: You must ensure synchronization yourself!
unsafe impl<T: Send> Sync for UnsafeSyncCell<T> {}
unsafe impl<T: Send> Send for UnsafeSyncCell<T> {}

lazy_static! {
    static ref SELF_CANISTER_ID: Principal =
        Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
    static ref SNS_LEDGER: Principal = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    static ref ICP_LEDGER: Principal = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    static ref SNS_ID: Principal = Principal::from_text("jg2ra-syaaa-aaaaq-aaewa-cai").unwrap();

    static ref TOKEN_0: String = format!("IC.{}", *SNS_LEDGER);
    static ref TOKEN_1: String = format!("IC.{}", *ICP_LEDGER);
    // Create test assets and request first
    static ref ASSET_0: Asset = Asset::Token {
        ledger_canister_id: *SNS_LEDGER,
        symbol: "DAO".to_string(),
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    static ref ASSET_1: Asset = Asset::Token {
        ledger_canister_id: *ICP_LEDGER,
        symbol: "ICP".to_string(),
        ledger_fee_decimals: Nat::from(FEE_ICP),
    };

    static ref OWNER_ACCOUNT: sns_treasury_manager::Account = sns_treasury_manager::Account {
        owner: Principal::from_text("2vxsx-fae").unwrap(),
        subaccount: None,
    };

}

lazy_static! {
    // Add fields to control mock behavior
    static ref CURRENT_STACK: UnsafeSyncCell<usize> = UnsafeSyncCell::new(0);
    static ref CURRENT_INSTRUCTION: UnsafeSyncCell<usize> = UnsafeSyncCell::new(0);
    static ref CALL_STACKS: UnsafeSyncCell<HashMap<usize, VecDeque<CallSpec>>> = UnsafeSyncCell::new(HashMap::<usize, VecDeque<CallSpec>>::new());
    // source stack id, source instruction id, destination stack id, destination instruction id
    static ref SWITCHING_CONTEXT: UnsafeSyncCell<Option<(usize, usize, usize, usize)>> = UnsafeSyncCell(UnsafeCell::new(None));

    static ref BARRIER: Barrier = Barrier::new(2);
}

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

#[derive(Clone)]
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

#[derive(Default)]
struct MockClock {
    time_ns: Arc<Mutex<u64>>,
}

impl MockClock {
    fn get_timer(&self) -> Box<dyn Fn() -> u64 + Send + Sync> {
        let time_ns = Arc::clone(&self.time_ns);
        Box::new(move || *time_ns.lock().unwrap())
    }
}

struct MockAgent {}

impl MockAgent {
    fn add_call<Req>(
        self,
        canister_id: Principal,
        request: Req,
        response: Req::Response,
        stack_id: usize,
    ) -> MockAgent
    where
        Req: Request,
    {
        let mut calls = VecDeque::new();

        let call = CallSpec::new(canister_id, request, response)
            .expect("Creating a new call specification failed");
        calls.push_back(call);

        let commit_state = CallSpec::new(*SELF_CANISTER_ID, CommitStateRequest {}, ())
            .expect("CommittState call creation failed");
        calls.push_back(commit_state);

        unsafe {
            (*CALL_STACKS.0.get())
                .entry(stack_id)
                .and_modify(|stack| stack.extend(calls.clone()))
                .or_insert(calls);
        }

        self
    }
}

impl AbstractAgent for MockAgent {
    type Error = MockError;
    // Infallable !
    async fn call<R: kongswap_adaptor::agent::Request + Debug + CandidType>(
        &self,
        canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> Result<R::Response, Self::Error> {
        let current_instruction = unsafe { *CURRENT_INSTRUCTION.0.get() };
        let current_stack = unsafe { *CURRENT_STACK.0.get() };
        println!(
            "started call: instruction #[{}], stack {}",
            current_instruction, current_stack
        );

        let Ok(raw_request) = request.payload() else {
            panic!("Cannot encode the request");
        };

        let expected_call = unsafe {
            (*CALL_STACKS.0.get())
                .get_mut(&current_stack)
                .unwrap()
                .pop_front()
                .expect("Consumed all expected requests")
        };

        if raw_request != expected_call.raw_request {
            println!("request: {:#?}", request);
            println!("{:?}\n{:?}", raw_request, expected_call.raw_request);
            panic!("Request doesn't match");
        }
        let canister_id: Principal = canister_id.into();

        assert_eq!(
            canister_id, expected_call.canister_id,
            "observed {canister_id}, expected {}",
            expected_call.canister_id
        );

        let reply = candid::decode_one::<R::Response>(&expected_call.raw_response)
            .expect("Unable to decode the response");

        println!("successfully called canister ID: {}", canister_id);

        unsafe {
            *(CURRENT_INSTRUCTION.0.get()) += 1;
        }

        let current_instruction = unsafe { *CURRENT_INSTRUCTION.0.get() };
        let current_stack = unsafe { *CURRENT_STACK.0.get() };

        let switching_context = unsafe { *SWITCHING_CONTEXT.0.get() };
        if let Some((
            source_stack,
            source_instruction,
            destination_stack,
            destination_instruction,
        )) = switching_context
        {
            if current_stack == source_stack && current_instruction == source_instruction {
                (*BARRIER).wait().await;
                unsafe {
                    *CURRENT_STACK.0.get() = destination_stack;
                    *CURRENT_INSTRUCTION.0.get() = destination_instruction;
                }

                unsafe {
                    *CURRENT_STACK.0.get() = source_stack;
                    *CURRENT_INSTRUCTION.0.get() = source_instruction;
                }
            }
        }

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

fn add_happy_deposit_calls(
    mock_agent: MockAgent,
    amount_0_decimals: u64,
    amount_1_decimals: u64,
    stack_id: usize,
) -> MockAgent {
    mock_agent
        .add_call(
            *SNS_LEDGER,
            make_approve_request(amount_0_decimals, FEE_SNS),
            Ok(Nat::from(amount_0_decimals)),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER,
            make_approve_request(amount_1_decimals, FEE_ICP),
            Ok(Nat::from(amount_1_decimals)),
            stack_id,
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_0_decimals - FEE_SNS),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_1_decimals - FEE_ICP),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(TOKEN_0.clone()),
            Ok(make_add_token_reply(
                1,
                "IC".to_string(),
                *SNS_ID,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(TOKEN_1.clone()),
            Ok(make_add_token_reply(
                2,
                "IC".to_string(),
                *ICP_LEDGER,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(TOKEN_0.clone()),
            Ok(make_update_token_reply(
                1,
                "IC".to_string(),
                *SNS_ID,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
            stack_id,
        )
        .add_call(
            *SNS_LEDGER,
            Icrc1MetadataRequest {},
            make_metadata_reply("My DAO Token".to_string(), "DAO".to_string(), FEE_SNS),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(TOKEN_1.clone()),
            Ok(make_update_token_reply(
                2,
                "IC".to_string(),
                *ICP_LEDGER,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER,
            Icrc1MetadataRequest {},
            make_metadata_reply("Internet Computer".to_string(), "ICP".to_string(), FEE_ICP),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_pool_request(
                TOKEN_0.clone(),
                amount_0_decimals - 2 * FEE_SNS,
                TOKEN_1.clone(),
                amount_1_decimals - 2 * FEE_ICP,
            ),
            Ok(AddPoolReply::default()),
            stack_id,
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER, // @todo
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
            stack_id,
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(TOKEN_0.clone()),
            Ok(make_update_token_reply(
                1,
                "IC".to_string(),
                *SNS_ID,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
            stack_id,
        )
        .add_call(
            *SNS_LEDGER,
            Icrc1MetadataRequest {},
            make_metadata_reply("My DAO Token".to_string(), "DAO".to_string(), FEE_SNS),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_update_token_request(TOKEN_1.clone()),
            Ok(make_update_token_reply(
                2,
                "IC".to_string(),
                *ICP_LEDGER,
                "Internet Computer".to_string(),
                "ICP".to_string(),
                FEE_ICP,
            )),
            stack_id,
        )
        .add_call(
            *ICP_LEDGER,
            Icrc1MetadataRequest {},
            make_metadata_reply("Internet Computer".to_string(), "ICP".to_string(), FEE_ICP),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_user_balances_request(*SELF_CANISTER_ID),
            Ok(vec![make_user_balance_reply()]),
            stack_id,
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_remove_liquidity_amounts_request(
                "DAO".to_string(),
                "ICP".to_string(),
                10000000000,
            ),
            Ok(make_remove_liquidity_amounts_reply(
                amount_0_decimals - 2 * FEE_SNS,
                amount_1_decimals - 2 * FEE_ICP,
            )),
            stack_id,
        )
}

// This test models the scenario, in which the second deposit call
// starts in the middle of the first deposit call.
#[tokio::test]
async fn test_lock_interleaving_should_not_pass() {
    println!("test: test_lock_interleaving_should_not_pass");
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
            asset: ASSET_0.clone(),
            owner_account: *OWNER_ACCOUNT,
            amount_decimals: Nat::from(amount_0_decimals),
        },
        // ICP
        Allowance {
            asset: ASSET_1.clone(),
            owner_account: *OWNER_ACCOUNT,
            amount_decimals: Nat::from(amount_1_decimals),
        },
    ];

    unsafe {
        (*SWITCHING_CONTEXT.0.get()) = Some((0, 2, 1, 0));
    }
    // Upon reaching the instruction id 1, we switch to the beginning of
    // the stack id 1.
    let mut mock_agent = MockAgent {};

    // We have two sequences of deposits.
    mock_agent = add_happy_deposit_calls(mock_agent, amount_0_decimals, amount_1_decimals, 0);
    mock_agent = add_happy_deposit_calls(mock_agent, amount_0_decimals, amount_1_decimals, 1);

    let mock_clock = MockClock::default();
    let mock_agent = Arc::new(mock_agent);

    let kong_adaptor = KongSwapAdaptor::new(
        mock_clock.get_timer(),
        Arc::clone(&mock_agent),
        *SELF_CANISTER_ID,
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

    struct UnsafeKongSwapAdaptor(UnsafeCell<KongSwapAdaptor<MockAgent>>);
    unsafe impl Sync for UnsafeKongSwapAdaptor {}
    unsafe impl Send for UnsafeKongSwapAdaptor {}

    let kong_adaptor_shared = Arc::new(UnsafeKongSwapAdaptor(UnsafeCell::new(kong_adaptor)));

    let t1 = {
        let kongswap_adaptor = Arc::clone(&kong_adaptor_shared);
        let allowances = allowances.clone();
        tokio::spawn(async move {
            unsafe {
                let kongswap_adaptor = &mut *kongswap_adaptor.0.get();
                let _ = kongswap_adaptor
                    .deposit(DepositRequest {
                        allowances: allowances.clone(),
                    })
                    .await;
            }
        })
    };

    let t2 = {
        let kongswap_adaptor = Arc::clone(&kong_adaptor_shared);
        let allowances = allowances.clone();
        tokio::spawn(async move {
            unsafe {
                let kongswap_adaptor = &mut *kongswap_adaptor.0.get();
                (*BARRIER).wait().await;
                let result = kongswap_adaptor
                    .deposit(DepositRequest {
                        allowances: allowances.clone(),
                    })
                    .await;

                let expected_error = Err(vec![sns_treasury_manager::Error {
                    code: 5, // temporary unavailable
                    message: "Canister state is locked. Please try again in 2700 seconds."
                        .to_string(),
                    kind: sns_treasury_manager::ErrorKind::TemporarilyUnavailable {},
                }]);
                assert_eq!(result, expected_error);
            }
        })
    };

    let _ = tokio::join!(t1, t2);
}
