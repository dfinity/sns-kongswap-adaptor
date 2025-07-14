use crate::{
    balances::ValidatedBalances,
    kong_types::{
        AddLiquidityAmountsArgs, AddLiquidityAmountsReply, AddLiquidityArgs, AddPoolArgs,
    },
    validation::{saturating_sub, ValidatedAllowance},
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::Nat;
use icrc_ledger_types::{icrc1::account::Account, icrc2::approve::ApproveArgs};
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::{Error, ErrorKind};

/// How many ledger transaction that incur fees are required for a deposit operation (per token).
/// This is an implementation detail of KongSwap and ICRC1 ledgers.
const DEPOSIT_LEDGER_FEES_PER_TOKEN: u64 = 2;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    /// Enforces that each KongSwapAdaptor instance manages a single token pair.
    pub(crate) fn validate_deposit_args(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Error> {
        let new_ledger_0 = allowance_0.asset.ledger_canister_id();
        let new_ledger_1 = allowance_1.asset.ledger_canister_id();

        let (old_asset_0, old_asset_1) = self.assets();

        if new_ledger_0 != old_asset_0.ledger_canister_id()
            || new_ledger_1 != old_asset_1.ledger_canister_id()
        {
            return Err(Error::new_precondition(format!(
                "This KongSwapAdaptor only supports {}:{} as token_{{0,1}} (got ledger_0 {}, ledger_1 {}).",
                old_asset_0.symbol(),
                old_asset_1.symbol(),
                new_ledger_0,
                new_ledger_1,
            )));
        }

        Ok(())
    }

    /// Set up the allowances for the KongSwapBackend canister.
    async fn set_dex_allowances(
        &mut self,
        context: &mut OperationContext,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Error> {
        for ValidatedAllowance {
            asset,
            amount_decimals,
            owner_account: _,
        } in [&allowance_0, &allowance_1]
        {
            let human_readable = format!(
                "Calling ICRC2 approve to set KongSwapBackend as spender for {}.",
                asset.symbol()
            );
            let canister_id = asset.ledger_canister_id();
            let fee_decimals = Nat::from(asset.ledger_fee_decimals());
            let fee = Some(fee_decimals.clone());
            let amount = Nat::from(amount_decimals.clone()) - fee_decimals;
            let request = ApproveArgs {
                from_subaccount: None,
                spender: Account {
                    owner: *KONG_BACKEND_CANISTER_ID,
                    subaccount: None,
                },
                // All approved tokens should be fully used up before the next deposit.
                amount,
                expected_allowance: Some(Nat::from(0u8)),
                // TODO: Choose a more concervative expiration date.
                expires_at: Some(u64::MAX),
                memo: None,
                created_at_time: None,
                fee,
            };

            // Fail early if at least one of the allowances fails.
            self.emit_transaction(
                context.next_operation(),
                canister_id,
                request,
                human_readable,
            )
            .await?;
        }

        Ok(())
    }

    async fn topup_pool(
        &mut self,
        context: &mut OperationContext,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Error> {
        let ledger_0 = allowance_0.asset.ledger_canister_id();
        let ledger_1 = allowance_1.asset.ledger_canister_id();

        // Adjust the amounts to reflect that `DEPOSIT_LEDGER_FEES_PER_TOKEN` transactions
        // (per token) are required for adding liquidity.
        //
        // The call to `validate_allowances` above ensures that the amounts are still
        // sufficiently large.
        let amount_0 = saturating_sub(
            Nat::from(allowance_0.amount_decimals),
            Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_0.asset.ledger_fee_decimals(),
        );
        // amount_1 is a function of amount_0.

        // Step 4. Ensure the pool exists.

        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        // This is a top-up operation for a pre-existing pool.
        // A top-up requires computing amount_1 as a function of amount_0.
        let AddLiquidityAmountsReply { amount_1, .. } = {
            let human_readable = format!(
                "Calling KongSwapBackend.add_liquidity_amounts to estimate how much liquidity can \
                 be added for token_1 ={} when adding token_0 = {}, amount_0 = {}.",
                token_1, token_0, amount_0,
            );
            let request = AddLiquidityAmountsArgs {
                token_0: token_0.clone(),
                amount: amount_0.clone(),
                token_1: token_1.clone(),
            };

            self.emit_transaction(
                context.next_operation(),
                *KONG_BACKEND_CANISTER_ID,
                request,
                human_readable,
            )
            .await?
        };

        let human_readable = format!(
            "Calling KongSwapBackend.add_liquidity to top up liquidity for \
                token_0 = {}, amount_0 = {}, token_1 = {}, amount_1 = {}.",
            token_0, amount_0, token_1, amount_1
        );

        let request = AddLiquidityArgs {
            token_0,
            amount_0,
            token_1,
            amount_1,

            // Not needed for the ICRC2 flow.
            tx_id_0: None,
            tx_id_1: None,
        };

        self.emit_transaction(
            context.next_operation(),
            *KONG_BACKEND_CANISTER_ID,
            request,
            human_readable,
        )
        .await?;

        Ok(())
    }

    fn is_pool_already_deployed_error(&self, message: String) -> bool {
        let lp_toke_symbol = self.lp_token();

        let tolerated_errors = [
            format!("LP token {} already exists", lp_toke_symbol),
            format!("Pool {} already exists", lp_toke_symbol),
        ];

        tolerated_errors.contains(&message)
    }

    async fn deposit_into_dex(
        &mut self,
        context: &mut OperationContext,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Vec<Error>> {
        self.set_dex_allowances(context, allowance_0, allowance_1)
            .await
            .map_err(|err| vec![err])?;

        let balances_before = self.get_ledger_balances(context).await?;

        let result = self.add_pool(context, allowance_0, allowance_1).await;

        if let Err(Error {
            kind: ErrorKind::Backend {},
            message,
            ..
        }) = result
        {
            if self.is_pool_already_deployed_error(message) {
                // If the pool already exists, we can proceed with a top-up.
                self.topup_pool(context, allowance_0, allowance_1)
                    .await
                    .map_err(|err| vec![err])?;
            }
        }

        let balances_after = self.get_ledger_balances(context).await?;

        self.find_discrepency(
            allowance_0.asset,
            balances_before.0,
            balances_after.0,
            allowance_0.amount_decimals,
            true,
        );
        self.find_discrepency(
            allowance_1.asset,
            balances_before.1,
            balances_after.1,
            allowance_1.amount_decimals,
            true,
        );

        Ok(())
    }

    async fn add_pool(
        &mut self,
        context: &mut OperationContext,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Error> {
        let ledger_0 = allowance_0.asset.ledger_canister_id();
        let ledger_1 = allowance_1.asset.ledger_canister_id();

        // Adjust the amounts to reflect that `DEPOSIT_LEDGER_FEES_PER_TOKEN` transactions
        // (per token) are required for adding liquidity.
        //
        // The call to `validate_allowances` above ensures that the amounts are still
        // sufficiently large.
        let amount_0 = saturating_sub(
            Nat::from(allowance_0.amount_decimals),
            Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_0.asset.ledger_fee_decimals(),
        );
        let amount_1 = saturating_sub(
            Nat::from(allowance_1.amount_decimals),
            Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_1.asset.ledger_fee_decimals(),
        );
        // Step 1. Ensure the tokens are registered with the DEX.
        // Notes on why we first add SNS and then ICP:
        // - KongSwap starts indexing tokens from 1.
        // - The ICP token is assumed to have index 2.
        // https://github.com/KongSwap/kong/blob/fe-predictions-update/src/kong_lib/src/ic/icp.rs#L1
        self.maybe_add_token(context, ledger_0).await?;
        self.maybe_add_token(context, ledger_1).await?;

        // Step 2. Fetch the latest ledger metadata, including symbols and ledger fees.
        // TODO: We could tolerate an error here and try to move forward.
        self.refresh_ledger_metadata(context).await?;

        // Step 3. Ensure the pool exists.

        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        self.emit_transaction(
            context.next_operation(),
            *KONG_BACKEND_CANISTER_ID,
            AddPoolArgs {
                token_0: token_0.clone(),
                amount_0: amount_0.clone(),
                token_1: token_1.clone(),
                amount_1,
                // Liquidity provider fee in basis points 30=0.3%.
                lp_fee_bps: Some(30),
                // Not needed for the ICRC2 flow.
                tx_id_0: None,
                tx_id_1: None,
            },
            "Calling KongSwapBackend.add_pool to add a new pool.".to_string(),
        )
        .await?;

        Ok(())
    }

    // TODO refersh balances
    pub async fn deposit_impl(
        &mut self,
        context: &mut OperationContext,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<ValidatedBalances, Vec<Error>> {
        {
            self.add_manager_balance(allowance_0.asset, allowance_0.amount_decimals);
            self.add_manager_balance(allowance_1.asset, allowance_1.amount_decimals);
        }

        let deposit_into_dex_result = self
            .deposit_into_dex(context, allowance_0, allowance_1)
            .await;

        let returned_amounts_result = self
            .return_remaining_assets_to_owner(
                context,
                allowance_0.owner_account,
                allowance_1.owner_account,
            )
            .await;

        match (deposit_into_dex_result, returned_amounts_result) {
            (Ok(_), Ok(_)) => Ok(self.get_cached_balances()),
            (Ok(_), Err(errs)) => Err(errs),
            (Err(errs), Ok(_)) => Err(errs),
            (Err(mut errs), Err(errs_1)) => {
                errs.extend(errs_1.into_iter());
                Err(errs)
            }
        }
    }
}

#[cfg(test)]
mod tests;
