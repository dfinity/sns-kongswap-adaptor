use super::*;
use crate::kong_types::{AddTokenArgs, AddTokenReply, ICReply};
use crate::tx_error_codes::TransactionErrorCodes;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use candid::Principal;
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, BalanceBook, Balances, BalancesRequest, DepositRequest, Step,
    TreasuryManager, TreasuryManagerInit, TreasuryManagerOperation,
};
use std::cell::RefCell;

const E8: u64 = 100_000_000;
const FEE_SNS: u64 = 10_500u64;
const FEE_ICP: u64 = 9_500u64;

lazy_static! {
    static ref OWNER_ACCOUNT: sns_treasury_manager::Account = sns_treasury_manager::Account {
        owner: Principal::from_text("2vxsx-fae").unwrap(),
        subaccount: None,
    };
    static ref MANAGER_ACCOUNT: sns_treasury_manager::Account = sns_treasury_manager::Account {
        owner: *SELF_CANISTER_ID,
        subaccount: None,
    };
}

use lazy_static::lazy_static;

lazy_static! {
    static ref SELF_CANISTER_ID: Principal =
        Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
    static ref MANAGER_NAME: String = format!("KongSwapAdaptor({})", *SELF_CANISTER_ID);
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
        expires_at: Some(ONE_HOUR),
        memo: None,
        created_at_time: None,
        fee: Some(fee.into()),
    }
}

fn make_balance_request() -> Account {
    Account {
        owner: *SELF_CANISTER_ID,
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

fn make_transfer_request(
    owner: Account,
    fee: u64,
    amount: u64,
    operation: TreasuryManagerOperation,
) -> TransferArg {
    TransferArg {
        from_subaccount: None,
        to: owner,
        fee: Some(Nat::from(fee)),
        created_at_time: Some(0),
        memo: Some(Memo::from(Vec::<u8>::from(operation))),
        amount: Nat::from(amount - fee),
    }
}

fn make_default_balance_book() -> BalanceBook {
    BalanceBook::empty()
        .with_treasury_owner(*OWNER_ACCOUNT, "DAO Treasury".to_string())
        .with_treasury_manager(*MANAGER_ACCOUNT, MANAGER_NAME.clone())
        .with_external_custodian(None, None)
        .with_suspense(None)
        .with_fee_collector(None, None)
        .with_payees(None, None)
        .with_payers(None, None)
}

#[tokio::test]
async fn test_failed_transfer_from_0() {
    let amount_0_decimals = 500 * E8;
    let amount_1_decimals = 400 * E8;

    let asset_0_balance = make_default_balance_book()
        .fee_collector(2 * FEE_SNS)
        .treasury_owner(amount_0_decimals - 2 * FEE_SNS);

    let asset_1_balance = make_default_balance_book()
        .fee_collector(2 * FEE_ICP)
        .treasury_owner(amount_1_decimals - 2 * FEE_ICP);

    run_failed_transfer_from_test(
        true,
        amount_0_decimals,
        amount_1_decimals,
        asset_0_balance,
        asset_1_balance,
    )
    .await;
}

#[tokio::test]
async fn test_failed_transfer_from_1() {
    let amount_0_decimals = 500 * E8;
    let amount_1_decimals = 400 * E8;

    let asset_0_balance = make_default_balance_book()
        .fee_collector(2 * FEE_SNS)
        .treasury_owner(amount_0_decimals - 4 * FEE_SNS)
        .treasury_manager(2 * FEE_SNS)
        .suspense(2 * FEE_SNS);

    let asset_1_balance = make_default_balance_book()
        .fee_collector(2 * FEE_ICP)
        .treasury_owner(amount_1_decimals - 2 * FEE_ICP);

    run_failed_transfer_from_test(
        false,
        amount_0_decimals,
        amount_1_decimals,
        asset_0_balance,
        asset_1_balance,
    )
    .await;
}

async fn run_failed_transfer_from_test(
    transfer_0_fails: bool,
    amount_0_decimals: u64,
    amount_1_decimals: u64,
    asset_0_balance: BalanceBook,
    asset_1_balance: BalanceBook,
) {
    let sns_ledger = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let icp_ledger = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();

    let token_0 = format!("IC.{}", sns_ledger);
    let token_1 = format!("IC.{}", icp_ledger);

    let symbol_0 = "DAO".to_string();
    let symbol_1 = "ICP".to_string();
    // Create test assets and request first
    let asset_0 = Asset::Token {
        ledger_canister_id: sns_ledger,
        symbol: symbol_0,
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    let asset_1 = Asset::Token {
        ledger_canister_id: icp_ledger,
        symbol: symbol_1,
        ledger_fee_decimals: Nat::from(FEE_ICP),
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
        // SNS
        Allowance {
            asset: asset_0.clone(),
            owner_account: *OWNER_ACCOUNT,
            amount_decimals: Nat::from(amount_0_decimals),
        },
        // ICP
        Allowance {
            asset: asset_1.clone(),
            owner_account: *OWNER_ACCOUNT,
            amount_decimals: Nat::from(amount_1_decimals),
        },
    ];

    // If transferring token0 fails, then from the perspective of
    // the treasury manager we have the exact same amount of balance
    // as before. Otherwise, if transferring token1 fails, then
    // kongswap backend has to send token0 back, with a round-trip
    // of fees deducted.
    let (balance_0_after_add_pool, error_message) = if transfer_0_fails {
        (
            amount_0_decimals - FEE_SNS,
            format!("Token_0 transfer failed"),
        )
    } else {
        (
            amount_0_decimals - 3 * FEE_SNS,
            format!("Token_1 transfer failed"),
        )
    };

    let mock_agent = MockAgent::new(*SELF_CANISTER_ID)
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
            make_balance_request(),
            Nat::from(amount_0_decimals - FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(token_0.clone()),
            Ok(make_add_token_reply(
                1,
                "IC".to_string(),
                sns_ledger,
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
            make_add_pool_request(
                token_0.clone(),
                amount_0_decimals - 2 * FEE_SNS,
                token_1.clone(),
                amount_1_decimals - 2 * FEE_ICP,
            ),
            Err(error_message.clone()),
        )
        .add_call(
            sns_ledger,
            make_balance_request(),
            Nat::from(balance_0_after_add_pool),
        )
        .add_call(
            icp_ledger, // @todo
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            sns_ledger,
            make_balance_request(),
            Nat::from(balance_0_after_add_pool),
        )
        .add_call(
            icp_ledger, // @todo
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            sns_ledger,
            make_transfer_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                FEE_SNS,
                balance_0_after_add_pool,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Deposit,
                    step: Step {
                        index: 11,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(balance_0_after_add_pool)),
        )
        .add_call(
            icp_ledger,
            make_transfer_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                FEE_ICP,
                amount_1_decimals - 1 * FEE_ICP,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Deposit,
                    step: Step {
                        index: 12,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(amount_1_decimals - 1 * FEE_ICP)),
        );

    let mut kong_adaptor = KongSwapAdaptor::new(
        || 0, // Mock time function
        mock_agent,
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

    // This should now work without panicking
    let result = kong_adaptor.deposit(DepositRequest { allowances }).await;

    assert!(
        kong_adaptor.agent.finished_calls(),
        "There are still some calls remaining"
    );

    assert_eq!(
        result,
        Err(vec![Error {
            code: TransactionErrorCodes::TemporaryUnavailableCode.into(),
            message: error_message,
            kind: ErrorKind::Backend {}
        }])
    );

    let balances = Balances {
        timestamp_ns: 0,
        asset_to_balances: Some(btreemap! {
            asset_0 => asset_0_balance,
            asset_1 => asset_1_balance,
        }),
    };

    let cached_balances = kong_adaptor.balances(BalancesRequest {});

    assert_eq!(cached_balances, Ok(balances));
}
