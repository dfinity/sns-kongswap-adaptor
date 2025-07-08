use crate::{
    balances::{Party, ValidatedBalances},
    kong_types::{ClaimArgs, ClaimsArgs, ClaimsReply, RemoveLiquidityArgs, RemoveLiquidityReply},
    log,
    tx_error_codes::TransactionErrorCodes,
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn withdraw_from_dex(&mut self) -> Result<(), Vec<TransactionError>> {
        let operation = TreasuryManagerOperation::new(sns_treasury_manager::Operation::Withdraw);

        let remove_lp_token_amount = self.lp_balance(operation).await.map_err(|err| vec![err])?;

        let human_readable =
            "Calling KongSwapBackend.remove_liquidity to withdraw all allocated tokens."
                .to_string();

        let (asset_0, asset_1) = self.assets();

        let request = RemoveLiquidityArgs {
            token_0: asset_0.symbol(),
            token_1: asset_1.symbol(),
            remove_lp_token_amount,
        };

        let RemoveLiquidityReply { claim_ids, .. } = self
            .emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?;

        // When removing the liquidity and withdrawing the tokens
        // from DEX to the treasury manager, we pay transfer fee.
        self.charge_fee(asset_0);
        self.charge_fee(asset_1);

        if !claim_ids.is_empty() {
            let claim_ids = claim_ids
                .iter()
                .map(|claim_id| claim_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(vec![TransactionError::Backend {
                error: format!(
                    "Withdrawal from DEX might not be complete, returned claims: {}.",
                    claim_ids
                ),
                code: u64::from(TransactionErrorCodes::BackendCode),
            }]);
        }

        Ok(())
    }

    pub async fn retry_withdraw_from_dex(&mut self) -> Result<(), Vec<TransactionError>> {
        let operation = TreasuryManagerOperation::new(sns_treasury_manager::Operation::Withdraw);

        let human_readable =
            "Calling KongSwapBackend.claims to check if a retry withdrawal is needed.".to_string();

        let claims = self
            .emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                ClaimsArgs {
                    principal_id: ic_cdk::api::id().to_string(),
                },
                operation,
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
                    *KONG_BACKEND_CANISTER_ID,
                    ClaimArgs { claim_id },
                    operation,
                    human_readable,
                )
                .await;

            // If withdrawal has previously failed and before retrying it,
            // the symbol of the asset changes, hence, we need to check the
            // ID of its corresponding ledger canister.
            match response {
                Ok(claim_reply) => {
                    self.with_balances_mut(|balances| {
                        let asset = if balances
                            .asset_0
                            .ledger_caniser_id_match(claim_reply.canister_id)
                        {
                            balances.asset_0
                        } else {
                            balances.asset_1
                        };

                        balances.charge_fee(&asset);
                    });
                }
                Err(err) => errors.push(err),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(())
    }

    pub async fn withdraw_impl(
        &mut self,
        withdraw_account_0: Account,
        withdraw_account_1: Account,
    ) -> Result<ValidatedBalances, Vec<TransactionError>> {
        let mut errors = vec![];

        if let Err(err) = self.withdraw_from_dex().await {
            errors.extend(err.into_iter());
        }

        if let Err(err) = self.retry_withdraw_from_dex().await {
            errors.extend(err.into_iter());
        }

        let returned_amounts = match self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::new(sns_treasury_manager::Operation::Withdraw),
                withdraw_account_0,
                withdraw_account_1,
            )
            .await
        {
            Ok(amounts) => Some(amounts),
            Err(err) => {
                errors.extend(err.into_iter());
                None
            }
        };

        // match self
        //     .get_ledger_balances(sns_treasury_manager::TreasuryManagerOperation::new(
        //         sns_treasury_manager::Operation::Withdraw,
        //     ))
        //     .await
        // {
        //     Ok((balance_0, balance_1)) => {
        //         self.with_balances_mut(|balances| {
        //             let asset = balances.asset_0.clone();
        //             balances.refresh_deposited_balances(&asset, balance_0, Party::TreasuryOwner)
        //         });
        //         self.with_balances_mut(|balances| {
        //             let asset = balances.asset_1.clone();
        //             balances.refresh_deposited_balances(&asset, balance_1, Party::TreasuryOwner)
        //         });
        //     }
        //     Err(tx_errors) => {
        //         errors.extend(tx_errors);
        //     }
        // }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(returned_amounts)
    }
}
