use super::*;
use crate::balances::ValidatedBalanceBook;
use crate::kong_types::{UserBalanceLPReply, UserBalancesArgs, UserBalancesReply};
use crate::validation::ValidatedAsset;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use candid::{Nat, Principal};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, BalanceBook, Balances, Step, TreasuryManager, TreasuryManagerInit,
    TreasuryManagerOperation, WithdrawRequest,
};
use std::cell::RefCell;

const E8: u64 = 100_000_000;

use lazy_static::lazy_static;

lazy_static! {
    static ref SELF_CANISTER_ID: Principal =
        Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
    static ref MANAGER_NAME: String = format!("KongSwapAdaptor({})", *SELF_CANISTER_ID);
}

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

fn make_balance_request() -> Account {
    Account {
        owner: *SELF_CANISTER_ID,
        subaccount: None,
    }
}

fn make_lp_balance_request() -> UserBalancesArgs {
    UserBalancesArgs {
        principal_id: SELF_CANISTER_ID.to_string(),
    }
}

fn make_lp_balance_reply(token_0: String, token_1: String, balance: f64) -> UserBalancesReply {
    UserBalancesReply::LP(UserBalanceLPReply {
        symbol: format!("{}_{}", token_0, token_1),
        name: String::default(),
        lp_token_id: 0,
        balance,
        usd_balance: 0.0,
        chain_0: String::default(),
        symbol_0: String::default(),
        address_0: String::default(),
        amount_0: 0.0,
        usd_amount_0: 0.0,
        chain_1: String::default(),
        symbol_1: String::default(),
        address_1: String::default(),
        amount_1: 0.0,
        usd_amount_1: 0.0,
        ts: 0,
    })
}

fn make_claims_reply(symbol_0: &String, symbol_1: &String) -> ClaimsReply {
    ClaimsReply {
        claim_id: 0,
        status: "Failed".to_string(),
        chain: "IC".to_string(),
        symbol: format!("{}_{}", symbol_0, symbol_1),
        canister_id: None,
        amount: Nat::from(0_u64),
        fee: Nat::from(0_u64),
        to_address: "".to_string(),
        desc: "".to_string(),
        ts: 0,
    }
}

fn make_claim_reply(
    symbol_0: &String,
    symbol_1: &String,
    ledger_id: String,
    amount: u64,
) -> ClaimReply {
    ClaimReply {
        claim_id: 0,
        status: "Success".to_string(),
        chain: "IC".to_string(),
        symbol: format!("{}_{}", symbol_0, symbol_1),
        canister_id: Some(ledger_id),
        amount: Nat::from(amount),
        fee: Nat::from(0_u64),
        to_address: "".to_string(),
        desc: "".to_string(),
        transfer_ids: vec![],
        ts: 0,
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

fn make_default_vallidated_balances(
    asset_0: &Asset,
    asset_1: &Asset,
    asset_0_balance: BalanceBook,
    asset_1_balance: BalanceBook,
) -> ValidatedBalances {
    ValidatedBalances {
        timestamp_ns: 0,
        asset_0: ValidatedAsset::try_from(asset_0.clone()).unwrap(),
        asset_1: ValidatedAsset::try_from(asset_1.clone()).unwrap(),
        asset_0_balance: ValidatedBalanceBook::try_from(asset_0_balance).unwrap(),
        asset_1_balance: ValidatedBalanceBook::try_from(asset_1_balance).unwrap(),
    }
}

// This test simulates the case, where a withdrawal has previously failed.
// This test shows that users can claim their failed withdrawals.
#[tokio::test]
async fn test_withdraw_retry() {
    const FEE_SNS: u64 = 10_500u64;
    const FEE_ICP: u64 = 9_500u64;
    let sns_ledger = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let icp_ledger = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();

    let symbol_0 = "DAO".to_string();
    let symbol_1 = "ICP".to_string();

    // Create test assets and request first
    let asset_0 = Asset::Token {
        ledger_canister_id: sns_ledger,
        symbol: symbol_0.clone(),
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    let asset_1 = Asset::Token {
        ledger_canister_id: icp_ledger,
        symbol: symbol_1.clone(),
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

    let amount_0_decimals = 500 * E8;
    let amount_1_decimals = 400 * E8;
    // We are going to create a claim for the token 0
    // with this amount. Upon a successful withdrawal
    // it would be deducted from the DEX.
    let amount_0_retry = 100 * E8;
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

    // Add deposit calls
    let mock_agent = MockAgent::new(*SELF_CANISTER_ID)
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_lp_balance_request(),
            Ok(vec![make_lp_balance_reply(
                symbol_0.clone(),
                symbol_1.clone(),
                0.0,
            )]),
        )
        .add_call(sns_ledger, make_balance_request(), Nat::from(0_u64))
        .add_call(icp_ledger, make_balance_request(), Nat::from(0_u64))
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            ClaimsArgs {
                principal_id: SELF_CANISTER_ID.to_string(),
            },
            Ok(vec![make_claims_reply(&symbol_0, &symbol_1)]),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            ClaimArgs { claim_id: 0 },
            Ok(make_claim_reply(
                &symbol_0,
                &symbol_1,
                sns_ledger.to_string(),
                amount_0_retry,
            )),
        )
        .add_call(
            sns_ledger,
            make_balance_request(),
            Nat::from(amount_0_retry - FEE_SNS),
        )
        .add_call(icp_ledger, make_balance_request(), Nat::from(0_u64))
        .add_call(
            sns_ledger,
            make_balance_request(),
            Nat::from(amount_0_retry - FEE_SNS),
        )
        .add_call(icp_ledger, make_balance_request(), Nat::from(0_u64))
        .add_call(
            sns_ledger,
            make_transfer_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                FEE_SNS,
                amount_0_retry - FEE_SNS,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Withdraw,
                    step: Step {
                        index: 9,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(amount_0_retry - 2 * FEE_SNS)),
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

    // We overwrite the balances to simulate the case after a
    // happy deposit.
    let asset_0_balance = make_default_balance_book()
        .fee_collector(2 * FEE_SNS)
        .external_custodian(amount_0_decimals - 2 * FEE_SNS);

    let asset_1_balance = make_default_balance_book()
        .fee_collector(2 * FEE_ICP)
        .external_custodian(amount_1_decimals - 2 * FEE_ICP);

    BALANCES.with_borrow_mut(|balances| {
        let validated_balances =
            make_default_vallidated_balances(&asset_0, &asset_1, asset_0_balance, asset_1_balance);

        balances
            .set(ConfigState::Initialized(validated_balances))
            .expect("Couldn't set the initial balances");
    });

    {
        let withdraw_accounts = btreemap! {
            sns_ledger => sns_treasury_manager::Account {
                owner: allowance_0.owner_account.owner,
                subaccount: None
            },
            icp_ledger => sns_treasury_manager::Account {
                owner: allowance_1.owner_account.owner,
                subaccount: None
            },
        };

        let result_withdraw = kong_adaptor
            .withdraw(WithdrawRequest {
                withdraw_accounts: Some(withdraw_accounts),
            })
            .await;

        // Check the correctness of the balances after claiming a
        // amount_0_retry from the DEX.
        // By a successful retrial, this amount is removed from the DEX,
        // and with a ledger transfer fee deducted from it(amount_0_retry - FEE_SNS)
        // it land in the treasury manager. Then, when returning all the
        // remaining assets to the owner, a second transfer fee would be deducted.
        let asset_0_balance = make_default_balance_book()
            .fee_collector(4 * FEE_SNS)
            .treasury_owner(amount_0_retry - 2 * FEE_SNS)
            .external_custodian(amount_0_decimals - 2 * FEE_SNS - amount_0_retry);

        let asset_1_balance = make_default_balance_book()
            .fee_collector(2 * FEE_ICP)
            .external_custodian(amount_1_decimals - 2 * FEE_ICP);

        let balances = Balances {
            timestamp_ns: 0,
            asset_to_balances: Some(btreemap! {
                asset_0 => asset_0_balance,
                asset_1 => asset_1_balance,
            }),
        };

        assert_eq!(result_withdraw, Ok(balances));
    }

    assert!(
        kong_adaptor.agent.finished_calls(),
        "There are still some calls remaining"
    );
}
