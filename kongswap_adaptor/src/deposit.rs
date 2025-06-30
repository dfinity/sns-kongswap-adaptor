use crate::{
    agent::AbstractAgent,
    emit_transaction::emit_transaction,
    kong_types::{
        AddLiquidityAmountsArgs, AddLiquidityAmountsReply, AddLiquidityArgs, AddLiquidityReply,
    },
    validation::{saturating_sub, ValidatedAllowance, ValidatedBalances},
    KongSwapAdaptor,
};
use candid::Nat;
use icrc_ledger_types::{icrc1::account::Account, icrc2::approve::ApproveArgs};
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

/// How many ledger transaction that incur fees are required for a deposit operation (per token).
/// This is an implementation detail of KongSwap and ICRC1 ledgers.
const DEPOSIT_LEDGER_FEES_PER_TOKEN: u64 = 2;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn deposit_into_dex(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<ValidatedBalances, TransactionError> {
        let operation = TreasuryManagerOperation::Deposit;

        // Step 0. Enforce that each KongSwapAdaptor instance manages a single token pair.
        {
            let new_ledger_0 = allowance_0.asset.ledger_canister_id();
            let new_ledger_1 = allowance_1.asset.ledger_canister_id();

            let old_asset_0 = self.balances.asset_0;
            let old_asset_1 = self.balances.asset_1;

            if new_ledger_0 != old_asset_0.ledger_canister_id()
                || new_ledger_1 != old_asset_1.ledger_canister_id()
            {
                return Err(TransactionError::Precondition(format!(
                    "This KongSwapAdaptor only supports {}:{} as token_{{0,1}} (got ledger_0 {}, ledger_1 {}).",
                    old_asset_0.symbol(),
                    old_asset_1.symbol(),
                    new_ledger_0,
                    new_ledger_1,
                )));
            }
        }

        // Step 1. Set up the allowances for the KongSwapBackend canister.
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
                    owner: self.kong_backend_canister_id,
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

            emit_transaction(
                &mut self.audit_trail,
                &self.agent,
                canister_id,
                request,
                operation,
                human_readable,
            )
            .await?;
        }

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

        // Step 2. Ensure the tokens are registered with the DEX.
        // Notes on why we first add SNS and then ICP:
        // - KongSwap starts indexing tokens from 1.
        // - The ICP token is assumed to have index 2.
        // https://github.com/KongSwap/kong/blob/fe-predictions-update/src/kong_lib/src/ic/icp.rs#L1
        self.maybe_add_token(ledger_0, operation).await?;
        self.maybe_add_token(ledger_1, operation).await?;

        // Step 3. Fetch the latest ledger metadata, including symbols and ledger fees.
        self.refresh_ledger_metadata(operation).await?;

        // Step 4. Ensure the pool exists.

        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        let original_amount_1 = amount_1.clone();

        let tolerated_errors = [
            format!("LP token {} already exists", self.lp_token()),
            format!("Pool {} already exists", self.lp_token()),
        ];

        match self
            .try_add_pool(
                &amount_0,
                &amount_1,
                ledger_0,
                ledger_1,
                allowance_0.owner_account,
                allowance_1.owner_account,
            )
            .await
        {
            Ok(balances) => {
                return Ok(balances);
            }
            // An already-existing pool does not preclude a top-up  =>  Keep going.
            Err(TransactionError::Backend(err)) if tolerated_errors.contains(&err) => (),

            Err(err) => {
                return Err(err);
            }
        }

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

            emit_transaction(
                &mut self.audit_trail,
                &self.agent,
                self.kong_backend_canister_id,
                request,
                operation,
                human_readable,
            )
            .await?
        };

        let reply = {
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

            emit_transaction(
                &mut self.audit_trail,
                &self.agent,
                self.kong_backend_canister_id,
                request,
                operation,
                human_readable,
            )
            .await?
        };

        let AddLiquidityReply {
            symbol_0,
            address_0,
            amount_0,
            symbol_1,
            amount_1,
            address_1,
            ..
        } = reply;

        if original_amount_1 < amount_1 {
            return Err(TransactionError::Backend(format!(
                "Got top-up amount_1 = {} (must be at least {})",
                original_amount_1, amount_1
            )));
        }

        self.reply_params_to_result(
            symbol_0,
            address_0,
            amount_0,
            allowance_0.owner_account,
            symbol_1,
            amount_1,
            address_1,
            allowance_1.owner_account,
        )
    }

    pub async fn deposit_impl(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<ValidatedBalances, Vec<TransactionError>> {
        let deposit_into_dex_result = self.deposit_into_dex(allowance_0, allowance_1).await;

        let returned_amounts_result = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::Withdraw,
                allowance_0.owner_account,
                allowance_1.owner_account,
            )
            .await;

        match (deposit_into_dex_result, returned_amounts_result) {
            (Ok(_), Ok(returned_amounts)) => Ok(returned_amounts),
            (Ok(_), Err(errs)) => Err(errs),
            (Err(err), Ok(_)) => Err(vec![err]),
            (Err(err_1), Err(errs_2)) => {
                let mut errs = vec![err_1];
                errs.extend(errs_2.into_iter());
                Err(errs)
            }
        }
    }
}
