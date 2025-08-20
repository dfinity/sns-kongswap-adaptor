use candid::{Nat, Principal};
use icrc_ledger_types::{
    icrc1::{
        account::Account,
        transfer::{Memo, TransferArg},
    },
    icrc2::{approve::ApproveArgs, transfer_from::TransferFromArgs},
};
use lazy_static::lazy_static;
use sns_treasury_manager::{Asset, BalanceBook, TreasuryManagerOperation};

use crate::{
    balances::{ValidatedBalanceBook, ValidatedBalances},
    deposit::ONE_HOUR,
    kong_types::{
        AddLiquidityAmountsArgs, AddLiquidityAmountsReply, AddLiquidityArgs, AddLiquidityReply,
        AddPoolArgs, AddPoolReply, AddTokenArgs, AddTokenReply, ClaimReply, ClaimsReply, ICReply,
        RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply, RemoveLiquidityArgs,
        RemoveLiquidityReply, UserBalanceLPReply, UserBalancesArgs, UserBalancesReply,
    },
    validation::{decode_nat_to_u64, ValidatedAsset},
    KONG_BACKEND_CANISTER_ID,
};

// Represents 10^8, commonly used for 8-decimal token amounts.
pub(crate) const E8: u64 = 100_000_000;
pub(crate) const FEE_SNS: u64 = 10_500u64;
pub(crate) const FEE_ICP: u64 = 9_500u64;

pub(crate) const IC: &'static str = "IC";
pub(crate) const SUCCESS_STATUS: &'static str = "Success";
pub(crate) const FAILURE_STATUS: &'static str = "Failed";

lazy_static! {
    pub(crate) static ref SELF_CANISTER_ID: Principal =
        Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
    pub(crate) static ref SNS_LEDGER: Principal = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();
    pub(crate) static ref ICP_LEDGER: Principal = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    pub(crate) static ref OWNER_ACCOUNT: sns_treasury_manager::Account = sns_treasury_manager::Account {
        owner: Principal::from_text("2vxsx-fae").unwrap(),
        subaccount: None,
    };
    pub(crate) static ref MANAGER_ACCOUNT: sns_treasury_manager::Account = sns_treasury_manager::Account {
        owner: *SELF_CANISTER_ID,
        subaccount: None,
    };

    pub(crate) static ref MANAGER_NAME: String = format!("KongSwapAdaptor({})", *SELF_CANISTER_ID);
    pub(crate) static ref TOKEN_0: String = format!("IC.{}", *SNS_LEDGER);
    pub(crate) static ref TOKEN_1: String = format!("IC.{}", *ICP_LEDGER);
    pub(crate) static ref SYMBOL_0: String = "DAO".to_string();
    pub(crate) static ref SYMBOL_1: String = "ICP".to_string();

    // Create test assets and request first
    pub(crate) static ref ASSET_0: Asset = Asset::Token {
        ledger_canister_id: *SNS_LEDGER,
        symbol: SYMBOL_0.clone(),
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    pub(crate) static ref ASSET_1: Asset = Asset::Token {
        ledger_canister_id: *ICP_LEDGER,
        symbol: SYMBOL_1.clone(),
        ledger_fee_decimals: Nat::from(FEE_ICP),
    };

}

pub(crate) fn make_approve_request(amount: u64, fee: u64) -> ApproveArgs {
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

pub(crate) fn make_balance_request() -> Account {
    Account {
        owner: *SELF_CANISTER_ID,
        subaccount: None,
    }
}

pub(crate) fn make_add_token_request(token: String) -> AddTokenArgs {
    AddTokenArgs { token }
}

pub(crate) fn make_add_token_reply(
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

pub(crate) fn make_add_pool_request(
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

pub(crate) fn make_add_pool_reply(token_0: &String, token_1: &String) -> AddPoolReply {
    AddPoolReply {
        status: SUCCESS_STATUS.to_string(),
        symbol_0: token_0.clone(),
        symbol_1: token_1.clone(),
        lp_token_symbol: format!("{}_{}", token_0, token_1),
        ..Default::default()
    }
}

pub(crate) fn make_default_balance_book() -> BalanceBook {
    BalanceBook::empty()
        .with_treasury_owner(*OWNER_ACCOUNT, "DAO Treasury".to_string())
        .with_treasury_manager(*MANAGER_ACCOUNT, MANAGER_NAME.clone())
        .with_external_custodian(None, None)
        .with_suspense(None)
        .with_fee_collector(None, None)
        .with_payees(None, None)
        .with_payers(None, None)
}

pub(crate) fn make_transfer_request(
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

pub(crate) fn make_lp_balance_request() -> UserBalancesArgs {
    UserBalancesArgs {
        principal_id: SELF_CANISTER_ID.to_string(),
    }
}

pub(crate) fn make_lp_balance_reply(
    token_0: String,
    token_1: String,
    balance: f64,
) -> UserBalancesReply {
    UserBalancesReply::LP(UserBalanceLPReply {
        symbol: format!("{}_{}", token_0, token_1),
        balance,
        ..Default::default()
    })
}

pub(crate) fn make_claims_reply(symbol_0: &String, symbol_1: &String) -> ClaimsReply {
    ClaimsReply {
        status: FAILURE_STATUS.to_string(),
        chain: IC.to_string(),
        symbol: format!("{}_{}", symbol_0, symbol_1),
        ..Default::default()
    }
}

pub(crate) fn make_claim_reply(
    symbol_0: &String,
    symbol_1: &String,
    ledger_id: String,
    amount: u64,
) -> ClaimReply {
    ClaimReply {
        status: SUCCESS_STATUS.to_string(),
        chain: IC.to_string(),
        symbol: format!("{}_{}", symbol_0, symbol_1),
        canister_id: Some(ledger_id),
        amount: Nat::from(amount),
        ..Default::default()
    }
}

pub(crate) fn make_remove_liquidity_request(
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

pub(crate) fn make_remove_liquidity_reply(
    token_0: String,
    token_1: String,
    amount_0: u64,
    amount_1: u64,
    lp_fee_0: u64,
    lp_fee_1: u64,
    remove_lp_token_amount: u64,
    claim_ids: Vec<u64>,
) -> RemoveLiquidityReply {
    RemoveLiquidityReply {
        status: SUCCESS_STATUS.to_string(),
        symbol: format!("{}_{}", token_0, token_1),
        symbol_0: token_0.clone(),
        amount_0: Nat::from(amount_0),
        lp_fee_0: Nat::from(lp_fee_0),
        symbol_1: token_1.clone(),
        amount_1: Nat::from(amount_1),
        lp_fee_1: Nat::from(lp_fee_1),
        remove_lp_token_amount: Nat::from(remove_lp_token_amount),
        transfer_ids: vec![],
        claim_ids,
        ts: 0,
        ..Default::default()
    }
}

pub(crate) fn make_default_validated_balances(
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

impl TryFrom<BalanceBook> for ValidatedBalanceBook {
    type Error = String;
    fn try_from(value: BalanceBook) -> Result<Self, Self::Error> {
        let mut errors = vec![];

        if value.treasury_owner.is_none() {
            errors.push("Missing treasury owner");
        };

        if value.treasury_manager.is_none() {
            errors.push("Missing treasury manager");
        }

        if !errors.is_empty() {
            return Err(errors.join(", "));
        }

        Ok(Self {
            treasury_owner: value.treasury_owner.unwrap().try_into().unwrap(),
            treasury_manager: value.treasury_manager.unwrap().try_into().unwrap(),
            external: value.external_custodian.map_or(0, |balance| {
                decode_nat_to_u64(balance.amount_decimals).unwrap_or(0)
            }),
            fee_collector: value.fee_collector.map_or(0, |balance| {
                decode_nat_to_u64(balance.amount_decimals).unwrap_or(0)
            }),
            spendings: value.payees.map_or(0, |balance| {
                decode_nat_to_u64(balance.amount_decimals).unwrap_or(0)
            }),
            earnings: value.payers.map_or(0, |balance| {
                decode_nat_to_u64(balance.amount_decimals).unwrap_or(0)
            }),
            suspense: value.suspense.map_or(0, |balance| {
                decode_nat_to_u64(balance.amount_decimals).unwrap_or(0)
            }),
        })
    }
}

pub(crate) fn make_add_liquidity_amounts_request(
    amount: u64,
    token_0: String,
    token_1: String,
) -> AddLiquidityAmountsArgs {
    AddLiquidityAmountsArgs {
        amount: Nat::from(amount),
        token_0,
        token_1,
    }
}

pub(crate) fn make_add_liquidity_amounts_reply(
    amount_0: u64,
    amount_1: u64,
    symbol_0: &String,
    symbol_1: &String,
) -> AddLiquidityAmountsReply {
    AddLiquidityAmountsReply {
        symbol_0: symbol_0.to_string(),
        amount_0: Nat::from(amount_0),
        symbol_1: symbol_1.to_string(),
        amount_1: Nat::from(amount_1),
        ..Default::default()
    }
}

pub(crate) fn make_add_liquidity_request(
    amount_0: u64,
    amount_1: u64,
    token_0: &String,
    token_1: &String,
) -> AddLiquidityArgs {
    AddLiquidityArgs {
        token_0: token_0.to_string(),
        amount_0: Nat::from(amount_0),
        tx_id_0: None,
        token_1: token_1.to_string(),
        amount_1: Nat::from(amount_1),
        tx_id_1: None,
    }
}

pub(crate) fn make_add_liquidity_reply(
    amount_0: u64,
    amount_1: u64,
    symbol_0: &String,
    symbol_1: &String,
) -> AddLiquidityReply {
    AddLiquidityReply {
        status: SUCCESS_STATUS.to_string(),
        symbol_0: symbol_0.to_string(),
        amount_0: Nat::from(amount_0),
        symbol_1: symbol_1.to_string(),
        amount_1: Nat::from(amount_1),
        ..Default::default()
    }
}

pub(crate) fn make_remove_liquidity_amount_request(
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

pub(crate) fn make_remove_liquidity_amount_reply(
    amount_0: u64,
    lp_fee_0: u64,
    amount_1: u64,
    lp_fee_1: u64,
    remove_lp_token_amount: u64,
) -> RemoveLiquidityAmountsReply {
    RemoveLiquidityAmountsReply {
        amount_0: Nat::from(amount_0),
        lp_fee_0: Nat::from(lp_fee_0),
        amount_1: Nat::from(amount_1),
        lp_fee_1: Nat::from(lp_fee_1),
        remove_lp_token_amount: Nat::from(remove_lp_token_amount),
        ..Default::default()
    }
}

pub(crate) fn make_transfer_from_request(
    from: Account,
    to: Account,
    fee: u64,
    amount: u64,
    operation: TreasuryManagerOperation,
) -> TransferFromArgs {
    TransferFromArgs {
        spender_subaccount: None,
        from,
        to,
        amount: Nat::from(amount),
        fee: Some(Nat::from(fee)),
        memo: Some(Memo::from(Vec::<u8>::from(operation))),
        created_at_time: None,
    }
}
