use std::fmt::Display;

use crate::{
    kong_types::{RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply, UpdateTokenArgs},
    log, log_err,
    tx_error_codes::TransactionErrorCodes,
    validation::{decode_nat_to_u64, ValidatedAsset, ValidatedBalance, ValidatedSymbol},
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::CandidType;
use icrc_ledger_types::{icrc::generic_metadata_value::MetadataValue, icrc1::account::Account};
use kongswap_adaptor::{
    agent::{icrc_requests::Icrc1MetadataRequest, AbstractAgent},
    audit::OperationContext,
};
use serde::Deserialize;
use sns_treasury_manager::{Error, ErrorKind};

#[allow(dead_code)]
/// This enumeration indicates which entity in our eco-system,
/// we are talking about. The naming Party is used to avoid confusion
/// with the term `Account`.
pub(crate) enum Party {
    TreasuryOwner,
    TreasuryManager,
    External,
    FeeCollector,
    Spendings,
    Earnings,
}

impl Display for Party {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Party::TreasuryOwner => write!(f, "TreasuryOwner"),
            Party::TreasuryManager => write!(f, "TreasuryManager"),
            Party::External => write!(f, "External"),
            Party::FeeCollector => write!(f, "FeeCollector"),
            Party::Earnings => write!(f, "Earning"),
            Party::Spendings => write!(f, "Spendings"),
        }
    }
}

#[derive(CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalanceBook {
    pub treasury_owner: ValidatedBalance,
    pub treasury_manager: ValidatedBalance,
    pub external: u64,
    pub fee_collector: u64,
    pub spendings: u64,
    pub earnings: u64,
    pub suspense: u64,
}

#[derive(CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalances {
    pub timestamp_ns: u64,
    pub asset_0: ValidatedAsset,
    pub asset_1: ValidatedAsset,
    pub asset_0_balance: ValidatedBalanceBook,
    pub asset_1_balance: ValidatedBalanceBook,
}

impl ValidatedBalances {
    pub(crate) fn new(
        timestamp_ns: u64,
        asset_0: ValidatedAsset,
        asset_1: ValidatedAsset,
        owner_name: String,
        owner_account_0: Account,
        owner_account_1: Account,
        manager_name: String,
        manager_account_0: Account,
        manager_account_1: Account,
    ) -> Self {
        let amount_decimals = 0;
        let external = 0;
        let fee_collector = 0;
        let spendings = 0;
        let earnings = 0;
        let suspense = 0;

        let asset_0_balance = ValidatedBalanceBook {
            treasury_owner: ValidatedBalance {
                amount_decimals,
                account: owner_account_0,
                name: owner_name.clone(),
            },
            treasury_manager: ValidatedBalance {
                amount_decimals,
                account: manager_account_0,
                name: manager_name.clone(),
            },
            external,
            fee_collector,
            spendings,
            earnings,
            suspense,
        };
        let asset_1_balance = ValidatedBalanceBook {
            treasury_owner: ValidatedBalance {
                amount_decimals,
                account: owner_account_1,
                name: owner_name,
            },
            treasury_manager: ValidatedBalance {
                amount_decimals,
                account: manager_account_1,
                name: manager_name,
            },
            external,
            fee_collector,
            spendings,
            earnings,
            suspense,
        };

        Self {
            timestamp_ns,
            asset_0,
            asset_1,
            asset_0_balance,
            asset_1_balance,
        }
    }

    // As the metadata of an asset, e.g., its symbol and fee, might change over time,
    // calling this function would update them.
    pub(crate) fn refresh_asset(&mut self, asset_id: usize, asset_new: ValidatedAsset) {
        let asset = if asset_id == 0 {
            &mut self.asset_0
        } else if asset_id == 1 {
            &mut self.asset_1
        } else {
            log_err(&format!("Invalid asset_id {}: must be 0 or 1.", asset_id));
            return;
        };

        let asset_old_info = asset.clone();

        let ValidatedAsset::Token {
            symbol: new_symbol,
            ledger_canister_id: _,
            ledger_fee_decimals: new_ledger_fee_decimals,
        } = asset_new;

        if asset.set_symbol(new_symbol) {
            log(&format!(
                "Changed asset_{} symbol from `{}` to `{}`.",
                asset_id,
                asset_old_info.symbol(),
                new_symbol,
            ));
            return;
        }

        if asset.set_ledger_fee_decimals(new_ledger_fee_decimals) {
            log(&format!(
                "Changed asset_{} ledger_fee_decimals from `{}` to `{}`.",
                asset_id,
                asset_old_info.ledger_fee_decimals(),
                new_ledger_fee_decimals,
            ));
        }
    }

    // This function updates the distribution of balances for
    // a given asset held by the external protocol.
    pub(crate) fn set_external_custodian_balance(&mut self, asset: ValidatedAsset, balance: u64) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        balance_book.external = balance;
    }

    pub(crate) fn add_manager_balance(&mut self, asset: ValidatedAsset, amount: u64) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        balance_book.treasury_manager.amount_decimals += amount;
    }

    // TODO[ATG]: Let's discuss this in detail.
    pub(crate) fn move_asset(
        &mut self,
        asset: ValidatedAsset,
        from: Party,
        to: Party,
        amount: u64,
    ) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        match (&from, &to) {
            (Party::External, Party::TreasuryManager) => {
                balance_book.external -= amount;
                balance_book.treasury_manager.amount_decimals +=
                    amount - asset.ledger_fee_decimals();
            }
            (Party::TreasuryManager, Party::TreasuryOwner) => {
                balance_book.treasury_manager.amount_decimals -= amount;
                balance_book.treasury_owner.amount_decimals += amount - asset.ledger_fee_decimals();
            }
            (Party::TreasuryManager, Party::External) => {
                balance_book.external += amount - asset.ledger_fee_decimals();
                balance_book.treasury_manager.amount_decimals -= amount;
            }
            _ => {
                log_err(&format!("Invalid asset movement from {} to {}", from, to));
            }
        }

        balance_book.fee_collector += asset.ledger_fee_decimals();
    }

    pub(crate) fn charge_fee(&mut self, asset: ValidatedAsset) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        let fee = asset.ledger_fee_decimals();
        balance_book.fee_collector += fee;
        balance_book.treasury_manager.amount_decimals -= fee;
    }

    pub(crate) fn find_deposit_discrepency(
        &mut self,
        asset: ValidatedAsset,
        balance_before: u64,
        balance_after: u64,
        transferred_amount: u64,
    ) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        if balance_after.abs_diff(balance_before) > transferred_amount {
            balance_book.suspense += balance_before.abs_diff(balance_after) - transferred_amount;
        }
    }

    pub(crate) fn find_withdraw_discrepency(
        &mut self,
        asset: ValidatedAsset,
        balance_before: u64,
        balance_after: u64,
        transferred_amount: u64,
    ) {
        let balance_book = if asset == self.asset_0 {
            &mut self.asset_0_balance
        } else if asset == self.asset_1 {
            &mut self.asset_1_balance
        } else {
            log_err(&format!(
                "Invalid asset: must be {} or {}.",
                self.asset_0.symbol(),
                self.asset_1.symbol()
            ));
            return;
        };

        if balance_after.abs_diff(balance_before) < transferred_amount - asset.ledger_fee_decimals()
        {
            balance_book.suspense += balance_after
                .abs_diff(balance_before + transferred_amount - asset.ledger_fee_decimals());
        }
    }
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn refresh_ledger_metadata_impl(
        &mut self,
        context: &mut OperationContext,
        asset_id: usize,
        mut asset: ValidatedAsset,
    ) -> Result<ValidatedAsset, Error> {
        let ledger_canister_id = asset.ledger_canister_id();
        let old_asset = asset.clone();

        // Phase I. Tell KongSwap to refresh.
        {
            let human_readable = format!(
                "Calling KongSwapBackend.update_token for ledger #{} ({}).",
                asset_id, ledger_canister_id,
            );

            let token = format!("IC.{}", ledger_canister_id);

            let result = self
                .emit_transaction(
                    context.next_operation(),
                    *KONG_BACKEND_CANISTER_ID,
                    UpdateTokenArgs { token },
                    human_readable,
                )
                .await;

            if let Err(err) = result {
                log_err(&format!(
                    "Error while updating KongSwap token for ledger #{} ({}): {:?}",
                    asset_id, ledger_canister_id, err,
                ));
            };
        }

        // Phase II. Refresh the localy stored metadata.
        let human_readable = format!(
            "Refreshing metadata for ledger #{} ({}).",
            asset_id, ledger_canister_id,
        );

        let reply = self
            .emit_transaction(
                context.next_operation(),
                ledger_canister_id,
                Icrc1MetadataRequest {},
                human_readable,
            )
            .await?;

        // II.A. Extract and potentially update the symbol.
        let new_symbol = reply.iter().find_map(|(key, value)| {
            if key == "icrc1:symbol" {
                Some(value.clone())
            } else {
                None
            }
        });

        let Some(MetadataValue::Text(new_symbol)) = new_symbol else {
            return Err(Error {
                code: u64::from(TransactionErrorCodes::PostConditionCode),
                message: format!(
                    "Ledger {} icrc1_metadata response does not have an `icrc1:symbol`.",
                    ledger_canister_id
                ),
                kind: ErrorKind::Postcondition {},
            });
        };

        match ValidatedSymbol::try_from(new_symbol) {
            Ok(new_symbol) => {
                asset.set_symbol(new_symbol);
            }
            Err(err) => {
                log_err(&format!(
                    "Failed to validate `icrc1:symbol` ({}). Keeping the old symbol `{}`.",
                    err,
                    old_asset.symbol()
                ));
            }
        }

        // II.B. Refresh the ledger fee.
        let new_fee = reply.into_iter().find_map(|(key, value)| {
            if key == "icrc1:fee" {
                Some(value)
            } else {
                None
            }
        });

        let Some(MetadataValue::Nat(new_fee)) = new_fee else {
            return Err(Error {
                message: format!(
                    "Ledger {} icrc1_metadata response does not have an `icrc1:fee`.",
                    ledger_canister_id
                ),
                kind: ErrorKind::Postcondition {},
                code: u64::from(TransactionErrorCodes::PostConditionCode),
            });
        };

        match decode_nat_to_u64(new_fee) {
            Ok(new_fee_decimals) => {
                asset.set_ledger_fee_decimals(new_fee_decimals);
            }
            Err(err) => {
                log_err(&format!(
                    "Failed to decode `icrc1:fee` as Nat ({}). Keeping the old fee {}.",
                    err,
                    old_asset.ledger_fee_decimals()
                ));
            }
        }

        Ok(asset)
    }

    /// Refreshes the latest metadata for the managed assets, e.g., to update the symbols.
    pub async fn refresh_ledger_metadata(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Error> {
        let (asset_0, asset_1) = self.assets();

        let asset_0 = self
            .refresh_ledger_metadata_impl(context, 0, asset_0)
            .await?;

        let asset_1 = self
            .refresh_ledger_metadata_impl(context, 1, asset_1)
            .await?;

        self.with_balances_mut(|validated_balances| {
            validated_balances.refresh_asset(0, asset_0);
            validated_balances.refresh_asset(1, asset_1);
        });

        Ok(())
    }

    /// Attempts to refresh the external custodian balances for both managed assets.
    pub async fn refresh_balances_impl(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Error> {
        let remove_lp_token_amount = self.lp_balance(context).await;

        let human_readable = format!(
            "Calling KongSwapBackend.remove_liquidity_amounts to estimate how much liquidity can be removed for LP token amount {}.",
            remove_lp_token_amount
        );

        let (asset_0, asset_1) = self.assets();

        let request = RemoveLiquidityAmountsArgs {
            token_0: asset_0.symbol(),
            token_1: asset_1.symbol(),
            remove_lp_token_amount,
        };

        let reply = self
            .emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                request,
                human_readable,
            )
            .await?;

        let RemoveLiquidityAmountsReply {
            amount_0, amount_1, ..
        } = reply;

        let balance_0_decimals = decode_nat_to_u64(amount_0).map_err(|err| Error {
            code: u64::from(TransactionErrorCodes::PostConditionCode),
            message: err.clone(),
            kind: ErrorKind::Postcondition {},
        })?;
        let balance_1_decimals = decode_nat_to_u64(amount_1).map_err(|err| Error {
            code: u64::from(TransactionErrorCodes::PostConditionCode),
            message: err.clone(),
            kind: ErrorKind::Postcondition {},
        })?;

        self.with_balances_mut(|validated_balances| {
            validated_balances.set_external_custodian_balance(asset_0, balance_0_decimals);
            validated_balances.set_external_custodian_balance(asset_1, balance_1_decimals);
        });

        Ok(())
    }
}
