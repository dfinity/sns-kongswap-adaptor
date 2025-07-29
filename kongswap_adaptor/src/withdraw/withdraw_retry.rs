use super::*;
use crate::test_helper::*;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use candid::{Nat, Principal};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, Balances, Step, TreasuryManager, TreasuryManagerInit,
    TreasuryManagerOperation, WithdrawRequest,
};
use std::cell::RefCell;

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
            make_default_validated_balances(&asset_0, &asset_1, asset_0_balance, asset_1_balance);

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
