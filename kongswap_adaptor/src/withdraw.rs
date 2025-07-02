use crate::{
    kong_types::{ClaimArgs, ClaimsArgs, ClaimsReply, RemoveLiquidityArgs, RemoveLiquidityReply},
    validation::ValidatedBalances,
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use kongswap_adaptor::audit::OperationContext;
use sns_treasury_manager::{Operation, TransactionError};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn withdraw_from_dex(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Vec<TransactionError>> {
        let remove_lp_token_amount = self.lp_balance(context).await.map_err(|err| vec![err])?;

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
            return Err(vec![TransactionError::Backend(format!(
                "Withdrawal from DEX might not be complete, returned claims: {}.",
                claim_ids
            ))]);
        }

        Ok(())
    }

    pub async fn retry_withdraw_from_dex(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Vec<TransactionError>> {
        let human_readable =
            "Calling KongSwapBackend.claims to check if a retry withdrawal is needed.".to_string();

        let mut claims = self
            .emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                ClaimsArgs {
                    principal_id: self.id.to_string(),
                },
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?
            .into_iter()
            .peekable();

        let mut errors = vec![];

        while let Some(ClaimsReply {
            claim_id, symbol, ..
        }) = claims.next()
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
        let mut context = OperationContext::new(Operation::Withdraw);

        let mut errors = vec![];

        if let Err(err) = self.withdraw_from_dex(&mut context).await {
            errors.extend(err.into_iter());
        }

        if let Err(err) = self.retry_withdraw_from_dex(&mut context).await {
            errors.extend(err.into_iter());
        }

        let returned_amounts = match self
            .return_remaining_assets_to_owner(&mut context, withdraw_account_0, withdraw_account_1)
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
