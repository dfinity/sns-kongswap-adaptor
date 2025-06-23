use crate::{
    emit_transaction::emit_transaction,
    kong_api::reply_params_to_result,
    kong_types::{RemoveLiquidityArgs, RemoveLiquidityReply},
    validation::ValidatedBalances,
    KongSwapAdaptor,
};
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl KongSwapAdaptor {
    pub async fn withdraw_impl(&mut self) -> Result<ValidatedBalances, TransactionError> {
        let phase = TreasuryManagerOperation::Withdraw;

        let remove_lp_token_amount = self.lp_balance(phase).await?;

        let human_readable =
            "Calling KongSwapBackend.remove_liquidity to withdraw all allocated tokens."
                .to_string();

        let request = RemoveLiquidityArgs {
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

        let RemoveLiquidityReply {
            status,
            symbol_0,
            address_0,
            amount_0,
            symbol_1,
            amount_1,
            address_1,
            ..
        } = reply;

        reply_params_to_result(
            "remove_liquidity",
            status,
            symbol_0,
            address_0,
            amount_0,
            symbol_1,
            amount_1,
            address_1,
        )
    }
}
