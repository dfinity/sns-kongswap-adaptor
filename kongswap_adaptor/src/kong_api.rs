use crate::{
    kong_types::{
        kong_lp_balance_to_decimals, AddTokenArgs, UserBalanceLPReply, UserBalancesArgs,
        UserBalancesReply,
    },
    validation::{decode_nat_to_u64, ValidatedAsset, ValidatedBalances},
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use itertools::{Either, Itertools};
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::TransactionError;
use std::collections::BTreeMap;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn lp_token(&self) -> String {
        let (asset_0, asset_1) = self.assets();
        format!("{}_{}", asset_0.symbol(), asset_1.symbol())
    }

    pub async fn maybe_add_token(
        &mut self,
        context: &mut OperationContext,
        ledger_canister_id: Principal,
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
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                request,
                human_readable,
            )
            .await;

        match response {
            Ok(_) => Ok(()),
            Err(TransactionError::Backend(err))
                if err == format!("Token {} already exists", token) =>
            {
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub async fn lp_balance(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<Nat, TransactionError> {
        let request = UserBalancesArgs {
            principal_id: self.id.to_string(),
        };

        let human_readable =
            "Calling KongSwapBackend.user_balances to get LP balances.".to_string();

        let replies = self
            .emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                request,
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
            return Err(TransactionError::Backend(format!(
                "Failed to convert balances: {:?}",
                errors.join(", ")
            )));
        }

        let lp_token = self.lp_token();

        let Some((_, balance)) = balances.into_iter().find(|(token, _)| *token == lp_token) else {
            return Err(TransactionError::Backend(format!(
                "Failed to get LP balance for {}.",
                lp_token
            )));
        };

        Ok(balance)
    }

    pub(crate) fn reply_params_to_result(
        &self,
        symbol_0: String,
        address_0: String,
        amount_0: Nat,
        owner_account_0: Account,
        symbol_1: String,
        amount_1: Nat,
        address_1: String,
        owner_account_1: Account,
    ) -> Result<ValidatedBalances, TransactionError> {
        let (fee_0, fee_1) = self.fees();

        let asset_0 = ValidatedAsset::try_from((symbol_0, address_0, fee_0))
            .map_err(TransactionError::Postcondition)?;

        let asset_1 = ValidatedAsset::try_from((symbol_1, address_1, fee_1))
            .map_err(TransactionError::Postcondition)?;

        let balance_0_decimals =
            decode_nat_to_u64(amount_0).map_err(TransactionError::Postcondition)?;
        let balance_1_decimals =
            decode_nat_to_u64(amount_1).map_err(TransactionError::Postcondition)?;

        Ok(ValidatedBalances::new(
            asset_0,
            asset_1,
            balance_0_decimals,
            balance_1_decimals,
            owner_account_0,
            owner_account_1,
        ))
    }
}
