use super::*;
use crate::kong_types::{
    AddPoolArgs, AddPoolReply, AddTokenArgs, AddTokenReply, ICReply, UserBalanceLPReply,
    UserBalancesArgs, UserBalancesReply,
};
use crate::{
    state::storage::ConfigState, validation::ValidatedTreasuryManagerInit, StableAuditTrail,
    StableBalances, AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
};
use candid::{Nat, Principal};
use ic_stable_structures::memory_manager::MemoryManager;
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use icrc_ledger_types::icrc2::approve::ApproveArgs;
use kongswap_adaptor::agent::mock_agent::MockAgent;
use maplit::btreemap;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{
    Allowance, Asset, Balance, BalanceBook, Balances, DepositRequest, Step, TreasuryManager,
    TreasuryManagerInit, TreasuryManagerOperation, WithdrawRequest,
};
use std::cell::RefCell;

const E8: u64 = 100_000_000;

use lazy_static::lazy_static;

lazy_static! {
    static ref SELF_CANISTER_ID: Principal =
        Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
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

fn make_lp_balance_request() -> UserBalancesArgs {
    UserBalancesArgs {
        principal_id: SELF_CANISTER_ID.to_string(),
    }
}

fn make_lp_balance_reply(token_0: String, token_1: String) -> UserBalancesReply {
    UserBalancesReply::LP(UserBalanceLPReply {
        symbol: format!("{}_{}", token_0, token_1),
        name: String::default(),
        lp_token_id: 0,
        balance: 100.0,
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

fn make_remove_liquidity_request(
    token_0: String,
    token_1: String,
    remove_lp_token_amount: u64,
) -> RemoveLiquidityArgs {
    RemoveLiquidityArgs {
        token_0,
        token_1,
        remove_lp_token_amount: Nat::from(remove_lp_token_amount),
    }
}

fn make_remove_liquidity_reply(
    token_0: String,
    token_1: String,
    amount_0: u64,
    amount_1: u64,
    lp_fee_0: u64,
    lp_fee_1: u64,
    remove_lp_token_amount: u64,
) -> RemoveLiquidityReply {
    RemoveLiquidityReply {
        tx_id: 0,
        request_id: 0,
        status: "Success".to_string(),
        symbol: format!("{}_{}", token_0, token_1),
        chain_0: String::default(),
        address_0: String::default(),
        symbol_0: token_0.clone(),
        amount_0: Nat::from(amount_0),
        lp_fee_0: Nat::from(lp_fee_0),
        chain_1: String::default(),
        address_1: String::default(),
        symbol_1: token_1.clone(),
        amount_1: Nat::from(amount_1),
        lp_fee_1: Nat::from(lp_fee_1),
        remove_lp_token_amount: Nat::from(remove_lp_token_amount),
        transfer_ids: vec![],
        claim_ids: vec![],
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

#[tokio::test]
async fn test_withdraw_success() {
    const FEE_SNS: u64 = 10_500u64;
    const FEE_ICP: u64 = 9_500u64;
    let sns_ledger = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    let icp_ledger = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    let sns_id = Principal::from_text("jg2ra-syaaa-aaaaq-aaewa-cai").unwrap();

    let token_0 = format!("IC.{}", sns_ledger);
    let token_1 = format!("IC.{}", icp_ledger);

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
            asset: asset_0.clone(),
            owner_account,
            amount_decimals: Nat::from(amount_0_decimals),
        },
        // ICP
        Allowance {
            asset: asset_1.clone(),
            owner_account,
            amount_decimals: Nat::from(amount_1_decimals),
        },
    ];

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
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_0_decimals - FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
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
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            icp_ledger, // @todo
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_lp_balance_request(),
            Ok(vec![make_lp_balance_reply(
                symbol_0.clone(),
                symbol_1.clone(),
            )]),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(0_u64),
        )
        .add_call(
            *KONG_BACKEND_CANISTER_ID,
            make_remove_liquidity_request(symbol_0.clone(), symbol_1.clone(), 100 * E8),
            Ok(make_remove_liquidity_reply(
                symbol_0.clone(),
                symbol_1.clone(),
                amount_0_decimals - 2 * FEE_SNS,
                amount_1_decimals - 2 * FEE_ICP,
                0,
                0,
                100,
            )),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_0_decimals - 3 * FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_1_decimals - 3 * FEE_ICP),
        )
        .add_call(
            sns_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_0_decimals - 3 * FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
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
            sns_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_0_decimals - 3 * FEE_SNS),
        )
        .add_call(
            icp_ledger,
            make_balance_request(*SELF_CANISTER_ID),
            Nat::from(amount_1_decimals - 3 * FEE_ICP),
        )
        .add_call(
            sns_ledger,
            make_transfer_request(
                Account {
                    owner: owner_account.owner,
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
            icp_ledger,
            make_transfer_request(
                Account {
                    owner: owner_account.owner,
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

    {
        // This should now work without panicking
        let result_deposit = kong_adaptor.deposit(DepositRequest { allowances }).await;

        // Check the correctness of the balances after deposit
        let mut asset_0_balance = BalanceBook::empty()
            .with_treasury_owner(owner_account, "DAO Treasury".to_string())
            .with_treasury_manager(
                sns_treasury_manager::Account {
                    owner: kong_adaptor.id,
                    subaccount: None,
                },
                format!("KongSwapAdaptor({})", kong_adaptor.id),
            )
            .with_external_custodian(None, None)
            .with_suspense(None)
            .with_fee_collector(None, None)
            .fee_collector(2 * FEE_SNS)
            .external_custodian(amount_0_decimals - 2 * FEE_SNS);

        asset_0_balance.payees = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });
        asset_0_balance.payers = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });

        let mut asset_1_balance = BalanceBook::empty()
            .with_treasury_owner(owner_account, "DAO Treasury".to_string())
            .with_treasury_manager(
                sns_treasury_manager::Account {
                    owner: kong_adaptor.id,
                    subaccount: None,
                },
                format!("KongSwapAdaptor({})", kong_adaptor.id),
            )
            .with_external_custodian(None, None)
            .with_suspense(None)
            .with_fee_collector(None, None)
            .fee_collector(2 * FEE_ICP)
            .external_custodian(amount_1_decimals - 2 * FEE_ICP);

        asset_1_balance.payees = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });
        asset_1_balance.payers = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });

        let balances = Balances {
            timestamp_ns: 0,
            asset_to_balances: Some(btreemap! {
                asset_0.clone() => asset_0_balance,
                asset_1.clone() => asset_1_balance,
            }),
        };

        assert_eq!(result_deposit, Ok(balances));
    }

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

        // Check the correctness of the balances after withdrawal
        let mut asset_0_balance = BalanceBook::empty()
            .with_treasury_owner(owner_account, "DAO Treasury".to_string())
            .with_treasury_manager(
                sns_treasury_manager::Account {
                    owner: kong_adaptor.id,
                    subaccount: None,
                },
                format!("KongSwapAdaptor({})", kong_adaptor.id),
            )
            .with_external_custodian(None, None)
            .with_suspense(None)
            .with_fee_collector(None, None)
            .fee_collector(4 * FEE_SNS)
            .treasury_owner(amount_0_decimals - 4 * FEE_SNS);

        asset_0_balance.payees = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });
        asset_0_balance.payers = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });

        let mut asset_1_balance = BalanceBook::empty()
            .with_treasury_owner(owner_account, "DAO Treasury".to_string())
            .with_treasury_manager(
                sns_treasury_manager::Account {
                    owner: kong_adaptor.id,
                    subaccount: None,
                },
                format!("KongSwapAdaptor({})", kong_adaptor.id),
            )
            .with_external_custodian(None, None)
            .with_suspense(None)
            .with_fee_collector(None, None)
            .fee_collector(4 * FEE_ICP)
            .treasury_owner(amount_1_decimals - 4 * FEE_ICP);

        asset_1_balance.payees = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });
        asset_1_balance.payers = Some(Balance {
            amount_decimals: 0_u64.into(),
            account: None,
            name: None,
        });

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
