use crate::{
    balances::{Party, ValidatedBalances},
    kong_types::{ClaimArgs, ClaimsArgs, ClaimsReply, RemoveLiquidityArgs, RemoveLiquidityReply},
    tx_error_codes::TransactionErrorCodes,
    validation::decode_nat_to_u64,
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{Error, ErrorKind, TreasuryManager, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn withdraw_from_dex(&mut self) -> Result<(), Vec<Error>> {
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

        let balances_before = self.get_ledger_balances(operation).await?;

        let RemoveLiquidityReply {
            claim_ids,
            amount_0,
            lp_fee_0,
            amount_1,
            lp_fee_1,
            ..
        } = self
            .emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
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
        let balances_after = self.get_ledger_balances(operation).await?;
        let amount_0 = decode_nat_to_u64(amount_0 + lp_fee_0).unwrap();
        let amount_1 = decode_nat_to_u64(amount_1 + lp_fee_1).unwrap();
        self.find_discrepency(
            &asset_0,
            balances_before.0,
            balances_after.0,
            amount_0,
            false,
        );
        self.find_discrepency(
            &asset_1,
            balances_before.1,
            balances_after.1,
            amount_1,
            false,
        );
        self.move_asset(&asset_0, amount_0, Party::External, Party::TreasuryManager);
        self.move_asset(&asset_1, amount_1, Party::External, Party::TreasuryManager);

        Ok(())
    }

    pub async fn retry_withdraw_from_dex(&mut self) -> Result<(), Vec<Error>> {
        let operation = TreasuryManagerOperation::new(sns_treasury_manager::Operation::Withdraw);

        let human_readable =
            "Calling KongSwapBackend.claims to check if a retry withdrawal is needed.".to_string();

        let balances_before = self.get_ledger_balances(operation).await?;
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

            let balances_after = self.get_ledger_balances(operation).await?;
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

                        let amount = decode_nat_to_u64(claim_reply.amount).unwrap();
                        balances.move_asset(
                            &asset,
                            Party::External,
                            Party::TreasuryManager,
                            amount,
                        );
                        balances.find_withdraw_discrepency(
                            &asset,
                            balances_before.0,
                            balances_after.0,
                            amount,
                        );
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
    ) -> Result<ValidatedBalances, Vec<Error>> {
        let mut errors = vec![];

        if let Err(err) = self.withdraw_from_dex().await {
            errors.extend(err.into_iter());
        }

        if let Err(err) = self.retry_withdraw_from_dex().await {
            errors.extend(err.into_iter());
        }

        match self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::new(sns_treasury_manager::Operation::Withdraw),
                withdraw_account_0,
                withdraw_account_1,
            )
            .await
        {
            Ok(_) => {}
            Err(err) => {
                errors.extend(err.clone());
                return Err(err);
            }
        };

        self.refresh_balances().await;
        Ok(self.get_cached_balances())
    }
}
