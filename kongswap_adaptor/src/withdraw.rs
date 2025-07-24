use crate::{
    balances::{Party, ValidatedBalances},
    kong_types::{
        ClaimArgs, ClaimReply, ClaimsArgs, ClaimsReply, RemoveLiquidityArgs, RemoveLiquidityReply,
    },
    tx_error_codes::TransactionErrorCodes,
    validation::decode_nat_to_u64,
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::Nat;
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::{Error, ErrorKind};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn withdraw_from_dex(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Vec<Error>> {
        let remove_lp_token_amount = self.lp_balance(context).await;

        if remove_lp_token_amount == Nat::from(0u8) {
            // Nothing to withdraw.
            return Ok(());
        }

        let human_readable =
            "Calling KongSwapBackend.remove_liquidity to withdraw all allocated tokens."
                .to_string();

        let (asset_0, asset_1) = self.assets();

        let request = RemoveLiquidityArgs {
            token_0: asset_0.symbol(),
            token_1: asset_1.symbol(),
            remove_lp_token_amount,
        };

        let balances_before = self.get_ledger_balances(context).await?;

        let RemoveLiquidityReply {
            claim_ids,
            amount_0,
            lp_fee_0,
            amount_1,
            lp_fee_1,
            ..
        } = self
            .emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                request,
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?;

        if !claim_ids.is_empty() {
            let claim_ids = claim_ids
                .iter()
                .map(|claim_id| claim_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(vec![Error {
                code: u64::from(TransactionErrorCodes::BackendCode),
                message: format!(
                    "Withdrawal from DEX might not be complete, returned claims: {}.",
                    claim_ids
                ),
                kind: ErrorKind::Backend {},
            }]);
        }

        // TODO Unwrapping
        let balances_after = self.get_ledger_balances(context).await?;

        let amount_0 = decode_nat_to_u64(amount_0 + lp_fee_0).unwrap();
        let amount_1 = decode_nat_to_u64(amount_1 + lp_fee_1).unwrap();

        self.find_discrepency(
            asset_0,
            balances_before.0,
            balances_after.0,
            amount_0,
            false,
        );
        self.find_discrepency(
            asset_1,
            balances_before.1,
            balances_after.1,
            amount_1,
            false,
        );
        self.move_asset(asset_0, amount_0, Party::External, Party::TreasuryManager);
        self.move_asset(asset_1, amount_1, Party::External, Party::TreasuryManager);

        Ok(())
    }

    pub async fn retry_withdraw_from_dex(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Vec<Error>> {
        let human_readable =
            "Calling KongSwapBackend.claims to check if a retry withdrawal is needed.".to_string();

        let balances_before = self.get_ledger_balances(context).await?;
        let claims = self
            .emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                ClaimsArgs {
                    principal_id: self.id.to_string(),
                },
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?;

        let mut errors = vec![];

        for ClaimsReply {
            claim_id, symbol, ..
        } in claims
        {
            let human_readable = format!(
                "Calling KongSwapBackend.claim to claim the liquidity for {}, claim ID {}.",
                symbol, claim_id,
            );

            let response = self
                .emit_transaction(
                    context.next_operation(),
                    *KONG_BACKEND_CANISTER_ID,
                    ClaimArgs { claim_id },
                    human_readable,
                )
                .await;

            let balances_after = self.get_ledger_balances(context).await?;
            // If withdrawal has previously failed and before retrying it,
            // the symbol of the asset changes, hence, we need to check the
            // ID of its corresponding ledger canister.
            match response {
                Ok(ClaimReply {
                    canister_id: Some(canister_id),
                    amount,
                    ..
                }) => {
                    if let Some(asset) = self.get_asset_for_ledger(&canister_id) {
                        match decode_nat_to_u64(amount) {
                            Ok(amount) => {
                                self.move_asset(
                                    asset,
                                    amount,
                                    Party::External,
                                    Party::TreasuryManager,
                                );
                                self.find_discrepency(
                                    asset,
                                    balances_before.0,
                                    balances_after.0,
                                    amount,
                                    false,
                                );
                            }
                            Err(err) => {
                                errors.push(Error::new_postcondition(format!(
                                    "Failed to decode amount for claim ID {}: {}",
                                    claim_id, err
                                )));
                            }
                        }
                    } else {
                        errors.push(Error::new_postcondition(format!(
                            "Cannot identify asset for ledger `{}` for claim ID {}",
                            canister_id, claim_id
                        )));
                    }
                }
                Ok(_) => {
                    errors.push(Error::new_postcondition(format!(
                        "Claim for claim ID {} returned no ledger canister ID.",
                        claim_id
                    )));
                }
                Err(err) => {
                    errors.push(err);
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(())
    }

    pub async fn withdraw_impl(
        &mut self,
        context: &mut OperationContext,
        withdraw_account_0: Account,
        withdraw_account_1: Account,
    ) -> Result<ValidatedBalances, Vec<Error>> {
        let mut errors = vec![];

        if let Err(err) = self.withdraw_from_dex(context).await {
            errors.extend(err.into_iter());
        }

        if let Err(err) = self.retry_withdraw_from_dex(context).await {
            errors.extend(err.into_iter());
        }

        match self
            .return_remaining_assets_to_owner(context, withdraw_account_0, withdraw_account_1)
            .await
        {
            Ok(_) => {}
            Err(err) => {
                errors.extend(err.clone());
                return Err(err);
            }
        };

        Ok(self.get_cached_balances())
    }
}

#[cfg(test)]
mod tests;
