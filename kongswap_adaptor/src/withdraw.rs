use crate::{
    agent::AbstractAgent,
    emit_transaction::emit_transaction,
    kong_types::{ClaimArgs, ClaimsArgs, ClaimsReply, RemoveLiquidityArgs, RemoveLiquidityReply},
    validation::ValidatedBalances,
    KongSwapAdaptor,
};
use icrc_ledger_types::icrc1::account::Account;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn withdraw_from_dex(&mut self) -> Result<(), Vec<TransactionError>> {
        let operation = TreasuryManagerOperation::Withdraw;

        let remove_lp_token_amount = self.lp_balance(operation).await.map_err(|err| vec![err])?;

        let human_readable =
            "Calling KongSwapBackend.remove_liquidity to withdraw all allocated tokens."
                .to_string();

        let request = RemoveLiquidityArgs {
            token_0: self.balances.asset_0.symbol(),
            token_1: self.balances.asset_1.symbol(),
            remove_lp_token_amount,
        };

        let RemoveLiquidityReply { claim_ids, .. } = emit_transaction(
            &mut self.audit_trail,
            &self.agent,
            self.kong_backend_canister_id,
            request,
            operation,
            human_readable,
        )
        .await
        .map_err(|err| vec![err])?;

        if claim_ids.is_empty() {
            return Ok(());
        }

        Ok(())
    }

    pub async fn retry_withdraw_from_dex(&mut self) -> Result<(), Vec<TransactionError>> {
        let operation = TreasuryManagerOperation::Withdraw;

        let human_readable =
            "Calling KongSwapBackend.claims to check if a retry withdrawal is needed.".to_string();

        let claims = emit_transaction(
            &mut self.audit_trail,
            &self.agent,
            self.kong_backend_canister_id,
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

            let response = emit_transaction(
                &mut self.audit_trail,
                &self.agent,
                self.kong_backend_canister_id,
                ClaimArgs { claim_id },
                operation,
                human_readable,
            )
            .await;

            if let Err(err) = response {
                errors.push(err);
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
                TreasuryManagerOperation::Withdraw,
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

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(returned_amounts.unwrap())
    }
}
