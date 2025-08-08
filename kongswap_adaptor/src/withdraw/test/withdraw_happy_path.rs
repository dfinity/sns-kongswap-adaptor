use crate::kong_types::ClaimsArgs;
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
    Allowance, Balances, Step, TreasuryManager, TreasuryManagerInit, TreasuryManagerOperation,
    WithdrawRequest,
};
use std::cell::RefCell;

#[tokio::test]
async fn test_withdraw_success() {
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
                        ConfigState::Uninitialized
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
            *KONG_BACKEND_CANISTER_ID,
            make_lp_balance_request(),
            Ok(vec![make_lp_balance_reply(
                SYMBOL_0.clone(),
                SYMBOL_1.clone(),
                100.0,
            )]),
        )
        .add_call(*SNS_LEDGER, make_balance_request(), Nat::from(0_u64))
        .add_call(*ICP_LEDGER, make_balance_request(), Nat::from(0_u64))
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_remove_liquidity_request(SYMBOL_0.clone(), SYMBOL_1.clone(), 100 * E8),
            Ok(make_remove_liquidity_reply(
                SYMBOL_0.clone(),
                SYMBOL_1.clone(),
                amount_0_decimals - 2 * FEE_SNS,
                amount_1_decimals - 2 * FEE_ICP,
                0,
                0,
                100,
                vec![],
            )),
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
            ClaimsArgs {
                principal_id: SELF_CANISTER_ID.to_string(),
            },
            Ok(vec![]),
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
            *SNS_LEDGER,
            make_transfer_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                FEE_SNS,
                amount_0_decimals - 3 * FEE_SNS,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Withdraw,
                    step: Step {
                        index: 11,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(amount_0_decimals - 3 * FEE_SNS)),
        )
        .add_call(
            *ICP_LEDGER,
            make_transfer_request(
                Account {
                    owner: OWNER_ACCOUNT.owner,
                    subaccount: None,
                },
                FEE_ICP,
                amount_1_decimals - 3 * FEE_ICP,
                TreasuryManagerOperation {
                    operation: sns_treasury_manager::Operation::Withdraw,
                    step: Step {
                        index: 12,
                        is_final: false,
                    },
                },
            ),
            Ok(Nat::from(amount_1_decimals - 3 * FEE_ICP)),
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
            make_default_validated_balances(&ASSET_0, &ASSET_1, asset_0_balance, asset_1_balance);

        balances
            .set(ConfigState::Initialized(validated_balances))
            .expect("Couldn't set the initial balances");
    });

    {
        let withdraw_accounts = btreemap! {
            *SNS_LEDGER => sns_treasury_manager::Account {
                owner: allowance_0.owner_account.owner,
                subaccount: None
            },
            *ICP_LEDGER => sns_treasury_manager::Account {
                owner: allowance_1.owner_account.owner,
                subaccount: None
            },
        };

        let result_withdraw = kong_adaptor
            .withdraw(WithdrawRequest {
                withdraw_accounts: Some(withdraw_accounts),
            })
            .await;

        // Check the correctness of the balances after withdrawal
        let asset_0_balance = make_default_balance_book()
            .fee_collector(4 * FEE_SNS)
            .treasury_owner(amount_0_decimals - 4 * FEE_SNS);

        let asset_1_balance = make_default_balance_book()
            .fee_collector(4 * FEE_ICP)
            .treasury_owner(amount_1_decimals - 4 * FEE_ICP);

        let balances = Balances {
            timestamp_ns: 0,
            asset_to_balances: Some(btreemap! {
                ASSET_0.clone() => asset_0_balance,
                ASSET_1.clone() => asset_1_balance,
            }),
        };

        assert_eq!(result_withdraw, Ok(balances));
    }
    assert!(
        kong_adaptor.agent.finished_calls(),
        "There are still some calls remaining"
    );
}
