use crate::state::KongSwapAdaptor;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use crate::{test_helpers::*, KONG_BACKEND_CANISTER_ID};
use candid::Nat;
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Balances, DepositRequest, Step, TreasuryManager, TreasuryManagerInit,
    TreasuryManagerOperation,
};
use std::cell::RefCell;

#[tokio::test]
async fn test_deposit_success() {
    let amount_0_decimals = 500 * E8;
    let amount_1_decimals = 400 * E8;

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

    let mock_agent = MockAgent::new(*SELF_CANISTER_ID)
        .add_call(
            *SNS_LEDGER,
            make_transfer_from_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                Account {
                    owner: *SELF_CANISTER_ID,
                    subaccount: None,
                },
                FEE_SNS,
                amount_0_decimals - 2 * FEE_SNS,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Deposit,
                    step: Step {
                        index: 0,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(0_u64)),
        )
        .add_call(
            *ICP_LEDGER,
            make_transfer_from_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                Account {
                    owner: *SELF_CANISTER_ID,
                    subaccount: None,
                },
                FEE_ICP,
                amount_1_decimals - 2 * FEE_ICP,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Deposit,
                    step: Step {
                        index: 1,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(1_u64)),
        )
        .add_call(
            *SNS_LEDGER,
            make_approve_request(amount_0_decimals - 2 * FEE_SNS, FEE_SNS),
            Ok(Nat::from(amount_0_decimals)),
        )
        .add_call(
            *ICP_LEDGER,
            make_approve_request(amount_1_decimals - 2 * FEE_ICP, FEE_ICP),
            Ok(Nat::from(amount_1_decimals)),
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(),
            Nat::from(amount_0_decimals - 3 * FEE_SNS),
        )
        .add_call(
            *ICP_LEDGER,
            make_balance_request(),
            Nat::from(amount_1_decimals - 3 * FEE_ICP),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_token_request(TOKEN_0.clone()),
            Ok(make_add_token_reply(
                1,
                "IC".to_string(),
                *SNS_LEDGER,
                "My DAO Token".to_string(),
                "DAO".to_string(),
                FEE_SNS,
            )),
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
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_add_pool_request(
                TOKEN_0.clone(),
                amount_0_decimals - 4 * FEE_SNS,
                TOKEN_1.clone(),
                amount_1_decimals - 4 * FEE_ICP,
            ),
            Ok(make_add_pool_reply(&SYMBOL_0, &SYMBOL_1)),
        )
        .add_call(*SNS_LEDGER, make_balance_request(), Nat::from(0_u64))
        .add_call(*ICP_LEDGER, make_balance_request(), Nat::from(0_u64))
        .add_call(*SNS_LEDGER, make_balance_request(), Nat::from(0_u64))
        .add_call(*ICP_LEDGER, make_balance_request(), Nat::from(0_u64));

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

    let asset_0_balance = make_default_balance_book()
        .fee_collector(2 * FEE_SNS)
        .external_custodian(amount_0_decimals - 4 * FEE_SNS);

    let asset_1_balance = make_default_balance_book()
        .fee_collector(2 * FEE_ICP)
        .external_custodian(amount_1_decimals - 4 * FEE_ICP);

    let balances = Balances {
        timestamp_ns: 0,
        asset_to_balances: Some(btreemap! {
            ASSET_0.clone() => asset_0_balance,
            ASSET_1.clone() => asset_1_balance,
        }),
    };

    assert_eq!(result, Ok(balances));
}
