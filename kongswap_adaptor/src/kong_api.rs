use crate::{
    kong_types::{
        kong_lp_balance_to_decimals, AddTokenArgs, UserBalanceLPReply, UserBalancesArgs,
        UserBalancesReply,
    },
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::{Nat, Principal};
use itertools::{Either, Itertools};
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};
use std::collections::BTreeMap;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn lp_token(&self) -> String {
        let (asset_0, asset_1) = self.assets();
        format!("{}_{}", asset_0.symbol(), asset_1.symbol())
    }

    pub async fn maybe_add_token(
        &mut self,
        ledger_canister_id: Principal,
        operation: TreasuryManagerOperation,
    ) -> Result<(), TransactionError> {
        let token = format!("IC.{}", ledger_canister_id);

        let human_readable = format!(
            "Calling KongSwapBackend.add_token to attempt to add {}.",
            token
        );

        let request = AddTokenArgs {
            token: token.clone(),
        };

        let response = self
            .emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
                human_readable,
            )
            .await;

        match response {
            Ok(_) => Ok(()),
            Err(TransactionError::Backend { error, code: _ })
                if error == format!("Token {} already exists", token) =>
            {
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub async fn lp_balance(
        &mut self,
        operation: TreasuryManagerOperation,
    ) -> Result<Nat, TransactionError> {
        let request = UserBalancesArgs {
            principal_id: ic_cdk::api::id().to_string(),
        };

        let human_readable =
            "Calling KongSwapBackend.user_balances to get LP balances.".to_string();

        let replies = self
            .emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
                human_readable,
            )
            .await?;

        if replies.is_empty() {
            return Ok(Nat::from(0_u8));
        }

        let (balances, errors): (BTreeMap<_, _>, Vec<_>) = replies.into_iter().partition_map(
            |UserBalancesReply::LP(UserBalanceLPReply {
                 symbol, balance, ..
             })| {
                match kong_lp_balance_to_decimals(balance) {
                    Ok(balance) => Either::Left((symbol, balance)),
                    Err(err) => {
                        Either::Right(format!("Failed to convert balance for {}: {}", symbol, err))
                    }
                }
            },
        );

        if !errors.is_empty() {
            return Err(TransactionError::Backend {
                error: format!("Failed to convert balances: {:?}", errors.join(", ")),
                code: 0,
            });
        }

        let lp_token = self.lp_token();

        let Some((_, balance)) = balances.into_iter().find(|(token, _)| *token == lp_token) else {
            return Err(TransactionError::Backend {
                error: format!("Failed to get LP balance for {}.", lp_token),
                code: 0,
            });
        };

        Ok(balance)
    }
}
