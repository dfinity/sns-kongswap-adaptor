use crate::{
    agent::icrc_requests::Icrc1MetadataRequest,
    emit_transaction::emit_transaction,
    kong_types::{RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply, UpdateTokenArgs},
    log,
    validation::{decode_nat_to_u64, ValidatedBalances, ValidatedSymbol},
    KongSwapAdaptor,
};
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl KongSwapAdaptor {
    pub fn get_cached_balances(&self) -> ValidatedBalances {
        self.balances.clone()
    }

    /// Refreshes the latest metadata for the managed assets, e.g., to update the symbols.
    pub async fn refresh_ledger_metadata(
        &mut self,
        phase: TreasuryManagerOperation,
    ) -> Result<(), TransactionError> {
        // TODO: All calls in this loop could be started in parallel, and then `join_all`d.
        for (asset_index, asset) in [&mut self.balances.asset_0, &mut self.balances.asset_1]
            .into_iter()
            .enumerate()
        {
            let ledger_canister_id = asset.ledger_canister_id();

            // Phase I. Tell KongSwap to refresh.
            {
                let human_readable = format!(
                    "Calling KongSwapBackend.update_token for ledger #{} ({}).",
                    asset_index, ledger_canister_id,
                );

                let token = format!("IC.{}", ledger_canister_id);

                emit_transaction(
                    &mut self.audit_trail,
                    &self.agent,
                    self.kong_backend_canister_id,
                    UpdateTokenArgs { token },
                    phase,
                    human_readable,
                )
                .await?;
            }

            // Phase II. Refresh the localy stored metadata.
            let human_readable = format!(
                "Refreshing metadata for ledger #{} ({}).",
                asset_index, ledger_canister_id,
            );

            let reply = emit_transaction(
                &mut self.audit_trail,
                &self.agent,
                ledger_canister_id,
                Icrc1MetadataRequest {},
                phase,
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

            let new_symbol =
                ValidatedSymbol::try_from(new_symbol).map_err(TransactionError::Postcondition)?;

            let old_symbol = asset.symbol();

            if asset.set_symbol(new_symbol) {
                log(&format!(
                    "Changing ledger #{} ({}) symbol from `{}` to `{}`.",
                    asset_index, ledger_canister_id, old_symbol, new_symbol,
                ));
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

            let new_fee_decimals =
                decode_nat_to_u64(new_fee).map_err(TransactionError::Postcondition)?;

            let old_fee_decimals = asset.ledger_fee_decimals();

            if asset.set_ledger_fee_decimals(new_fee_decimals) {
                log(&format!(
                    "Changing ledger #{} ({}) fee_decimals from `{}` to `{}`.",
                    asset_index, ledger_canister_id, old_fee_decimals, new_fee_decimals,
                ));
            }
        }

        Ok(())
    }

    pub async fn refresh_balances(&mut self) -> Result<ValidatedBalances, TransactionError> {
        let phase = TreasuryManagerOperation::Balances;

        self.refresh_ledger_metadata(phase).await?;

        let remove_lp_token_amount = self.lp_balance(phase).await?;

        let human_readable = format!(
            "Calling KongSwapBackend.remove_liquidity_amounts to estimate how much liquidity can be removed for LP token amount {}.",
            remove_lp_token_amount
        );

        let request = RemoveLiquidityAmountsArgs {
            token_0: self.balances.asset_0.symbol(),
            token_1: self.balances.asset_1.symbol(),
            remove_lp_token_amount,
        };

        let reply = emit_transaction(
            &mut self.audit_trail,
            &self.agent,
            self.kong_backend_canister_id,
            request,
            phase,
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

        self.balances
            .set(balance_0_decimals, balance_1_decimals, ic_cdk::api::time());

        Ok(self.get_cached_balances())
    }
}
