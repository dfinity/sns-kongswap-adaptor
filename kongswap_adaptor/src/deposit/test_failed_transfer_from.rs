use super::*;
use crate::test_helper::*;
use crate::tx_error_codes::TransactionErrorCodes;
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, BalanceBook, Balances, BalancesRequest, DepositRequest, Step, TreasuryManager,
    TreasuryManagerInit, TreasuryManagerOperation,
};
use std::cell::RefCell;

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
            *SNS_LEDGER,
            make_approve_request(amount_0_decimals, FEE_SNS),
            Ok(Nat::from(amount_0_decimals)),
        )
        .add_call(
            *ICP_LEDGER,
            make_approve_request(amount_1_decimals, FEE_ICP),
            Ok(Nat::from(amount_1_decimals)),
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(),
            Nat::from(amount_0_decimals - FEE_SNS),
        )
        .add_call(
            *ICP_LEDGER,
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
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
                amount_0_decimals - 2 * FEE_SNS,
                TOKEN_1.clone(),
                amount_1_decimals - 2 * FEE_ICP,
            ),
            Err(error_message.clone()),
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(),
            Nat::from(balance_0_after_add_pool),
        )
        .add_call(
            *ICP_LEDGER, // @todo
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            *SNS_LEDGER,
            make_balance_request(),
            Nat::from(balance_0_after_add_pool),
        )
        .add_call(
            *ICP_LEDGER, // @todo
            make_balance_request(),
            Nat::from(amount_1_decimals - FEE_ICP),
        )
        .add_call(
            *SNS_LEDGER,
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
            *ICP_LEDGER,
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
            ASSET_0.clone() => asset_0_balance,
            ASSET_1.clone() => asset_1_balance,
        }),
    };

    let cached_balances = kong_adaptor.balances(BalancesRequest {});

    assert_eq!(cached_balances, Ok(balances));
}
