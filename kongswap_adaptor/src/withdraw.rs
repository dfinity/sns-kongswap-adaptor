use crate::{
    agent::AbstractAgent, emit_transaction::emit_transaction, kong_types::RemoveLiquidityArgs,
    validation::ValidatedBalances, KongSwapAdaptor,
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

        let _reply = emit_transaction(
            &mut self.audit_trail,
            &self.agent,
            self.kong_backend_canister_id,
            request,
            operation,
            human_readable,
        )
        .await
        .map_err(|err| vec![err])?;

        Ok(())
    }

    pub async fn withdraw_impl(
        &mut self,
        withdraw_account_0: Account,
        withdraw_account_1: Account,
    ) -> Result<ValidatedBalances, Vec<TransactionError>> {
        let withdraw_from_dex_result = self.withdraw_from_dex().await;

        let returned_amounts_result = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::Withdraw,
                withdraw_account_0,
                withdraw_account_1,
            )
            .await;

        match (withdraw_from_dex_result, returned_amounts_result) {
            (Ok(_), Ok(returned_amounts)) => Ok(returned_amounts),
            (Ok(_), Err(errs)) | (Err(errs), Ok(_)) => Err(errs),
            (Err(mut errs_1), Err(errs_2)) => {
                errs_1.extend(errs_2.into_iter());
                Err(errs_1)
            }
        }
    }
}
