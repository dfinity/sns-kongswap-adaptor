use crate::{
    kong_types::{RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply, UpdateTokenArgs},
    log_err,
    validation::{decode_nat_to_u64, ValidatedBalances, ValidatedSymbol},
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue;
use kongswap_adaptor::{
    agent::{icrc_requests::Icrc1MetadataRequest, AbstractAgent},
    audit::OperationContext,
};
use sns_treasury_manager::{Operation, TransactionError};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    /// Refreshes the latest metadata for the managed assets, e.g., to update the symbols.
    pub async fn refresh_ledger_metadata(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), TransactionError> {
        let (asset_0, asset_1) = self.assets();

        // TODO: All calls in this loop could be started in parallel, and then `join_all`d.

        let mut iter = [asset_0, asset_1].into_iter().enumerate().peekable();

        while let Some((asset_id, mut asset)) = iter.next() {
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
                return Err(TransactionError::Postcondition(format!(
                    "Ledger {} icrc1_metadata response does not have an `icrc1:symbol`.",
                    ledger_canister_id
                )));
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
                return Err(TransactionError::Postcondition(format!(
                    "Ledger {} icrc1_metadata response does not have an `icrc1:fee`.",
                    ledger_canister_id
                )));
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

            self.with_balances_mut(|balances| {
                balances.refresh_asset(asset_id, asset);
            });
        }

        Ok(())
    }

    pub async fn refresh_balances_impl(&mut self) -> Result<ValidatedBalances, TransactionError> {
        let mut context = OperationContext::new(Operation::Balances);

        if let Err(err) = self.refresh_ledger_metadata(&mut context).await {
            log_err(&format!("Failed to refresh ledger metadata: {:?}", err));
        }

        let remove_lp_token_amount = self.lp_balance(&mut context).await?;

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

        let balance_0_decimals =
            decode_nat_to_u64(amount_0).map_err(TransactionError::Postcondition)?;
        let balance_1_decimals =
            decode_nat_to_u64(amount_1).map_err(TransactionError::Postcondition)?;

        self.with_balances_mut(|balances| {
            balances.set(balance_0_decimals, balance_1_decimals, ic_cdk::api::time());
        });

        Ok(self.get_cached_balances())
    }
}
