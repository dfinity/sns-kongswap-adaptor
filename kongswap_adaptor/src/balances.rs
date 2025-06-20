use crate::{
    kong_types::{RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply},
    validation::{decode_nat_to_u64, ValidatedBalances},
    KongSwapAdaptor,
};
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl KongSwapAdaptor {
    pub fn get_cached_balances(&self) -> ValidatedBalances {
        self.balances.clone()
    }

    pub async fn refresh_balances(&mut self) -> Result<ValidatedBalances, TransactionError> {
        let phase = TreasuryManagerOperation::Balances;

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

        let reply = self
            .emit_transaction(
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
