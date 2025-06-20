use maplit::btreemap;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};
use std::collections::BTreeMap;

use crate::{
    kong_types::{RemoveLiquidityAmountsArgs, RemoveLiquidityAmountsReply},
    validation::{decode_nat_to_u64, ValidatedAsset},
    KongSwapAdaptor,
};

impl KongSwapAdaptor {
    pub fn get_cached_balances(&self) -> BTreeMap<ValidatedAsset, u64> {
        btreemap! {
            self.token_0 => self.balance_0_decimals.clone(),
            self.token_1 => self.balance_1_decimals.clone(),
        }
    }

    pub async fn refresh_balances(
        &mut self,
    ) -> Result<BTreeMap<ValidatedAsset, u64>, TransactionError> {
        let phase = TreasuryManagerOperation::Balances;

        let remove_lp_token_amount = self.lp_balance(phase).await?;

        let human_readable = format!(
            "Calling KongSwapBackend.remove_liquidity_amounts to estimate how much liquidity can be removed for LP token amount {}.",
            remove_lp_token_amount
        );

        let request = RemoveLiquidityAmountsArgs {
            token_0: self.token_0.symbol(),
            token_1: self.token_1.symbol(),
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

        let amount_0 = decode_nat_to_u64(amount_0).map_err(TransactionError::Postcondition)?;
        let amount_1 = decode_nat_to_u64(amount_1).map_err(TransactionError::Postcondition)?;

        self.balance_0_decimals = amount_0.clone();
        self.balance_1_decimals = amount_1.clone();

        Ok(self.get_cached_balances())
    }
}
