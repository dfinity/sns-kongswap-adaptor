use crate::{
    balances::{Party, ValidatedBalances},
    kong_types::{
        AddLiquidityAmountsArgs, AddLiquidityAmountsReply, AddLiquidityArgs, AddLiquidityReply,
        AddPoolArgs,
    },
    tx_error_codes::TransactionErrorCodes,
    validation::{decode_nat_to_u64, saturating_sub, ValidatedAllowance},
    KongSwapAdaptor, KONG_BACKEND_CANISTER_ID,
};
use candid::Nat;
use icrc_ledger_types::{icrc1::account::Account, icrc2::approve::ApproveArgs};
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{Error, ErrorKind, TreasuryManager, TreasuryManagerOperation};

/// How many ledger transaction that incur fees are required for a deposit operation (per token).
/// This is an implementation detail of KongSwap and ICRC1 ledgers.
const DEPOSIT_LEDGER_FEES_PER_TOKEN: u64 = 2;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    async fn deposit_into_dex(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<(), Vec<Error>> {
        let operation = TreasuryManagerOperation::new(sns_treasury_manager::Operation::Deposit);

        // Step 0. Enforce that each KongSwapAdaptor instance manages a single token pair.
        {
            let new_ledger_0 = allowance_0.asset.ledger_canister_id();
            let new_ledger_1 = allowance_1.asset.ledger_canister_id();

            let (old_asset_0, old_asset_1) = self.assets();

            if new_ledger_0 != old_asset_0.ledger_canister_id()
                || new_ledger_1 != old_asset_1.ledger_canister_id()
            {
                return Err(vec![Error {
                code: u64::from(TransactionErrorCodes::PreConditionCode),
                    message: format!(
                    "This KongSwapAdaptor only supports {}:{} as token_{{0,1}} (got ledger_0 {}, ledger_1 {}).",
                    old_asset_0.symbol(),
                    old_asset_1.symbol(),
                    new_ledger_0,
                    new_ledger_1,
                ),
                kind: ErrorKind::Precondition {  }
                }]
            );
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

            // Charge the approval fee.
            self.charge_fee(asset);

            self.emit_transaction(canister_id, request, operation, human_readable)
                .await
                .map_err(|err| vec![err])?;
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
        self.maybe_add_token(ledger_0, operation)
            .await
            .map_err(|err| vec![err])?;
        self.maybe_add_token(ledger_1, operation)
            .await
            .map_err(|err| vec![err])?;

        // Step 3. Fetch the latest ledger metadata, including symbols and ledger fees.
        self.refresh_ledger_metadata(operation)
            .await
            .map_err(|err| vec![err])?;

        // Step 4. Ensure the pool exists.
        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        let balances_before = self.get_ledger_balances(operation).await?;

        let result = self
            .emit_transaction(
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
                TreasuryManagerOperation::new(sns_treasury_manager::Operation::Deposit),
                "Calling KongSwapBackend.add_pool to add a new pool.".to_string(),
            )
            .await;

        let lp_toke_symbol = self.lp_token();

        let tolerated_errors = [
            format!("LP token {} already exists", lp_toke_symbol),
            format!("Pool {} already exists", lp_toke_symbol),
        ];

        match result {
            // All used up, since the pool is brand new.
            Ok(add_pool_reply) => {
                // Transferring the assets to DEX was successful.
                // Charge the transfer fee.
                // TODO unwrapping
                let amount_0 = decode_nat_to_u64(add_pool_reply.balance_0).unwrap();
                let amount_1 = decode_nat_to_u64(add_pool_reply.balance_1).unwrap();
                self.move_asset(
                    &allowance_0.asset,
                    amount_0,
                    Party::TreasuryManager,
                    Party::External,
                );
                self.move_asset(
                    &allowance_1.asset,
                    amount_1,
                    Party::TreasuryManager,
                    Party::External,
                );

                let balances_after = self.get_ledger_balances(operation).await?;
                self.find_discrepency(
                    &allowance_0.asset,
                    balances_before.0,
                    balances_after.0,
                    amount_0,
                    true,
                );
                self.find_discrepency(
                    &allowance_1.asset,
                    balances_before.1,
                    balances_after.1,
                    amount_1,
                    true,
                );

                return Ok(());
            }

            // An already-existing pool does not preclude a top-up  =>  Keep going.
            Err(Error { message, .. }) if tolerated_errors.contains(&message) => (),

            Err(err) => {
                return Err(vec![err]);
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

            self.emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?
        };

        let reply = {
            let human_readable = format!(
                "Calling KongSwapBackend.add_liquidity to top up liquidity for \
                 token_0 = {}, amount_0 = {}, token_1 = {}, amount_1 = {}.",
                token_0, amount_0, token_1, amount_1
            );

            let request = AddLiquidityArgs {
                token_0,
                amount_0: amount_0.clone(),
                token_1,
                amount_1: amount_1.clone(),

                // Not needed for the ICRC2 flow.
                tx_id_0: None,
                tx_id_1: None,
            };

            self.emit_transaction(
                *KONG_BACKEND_CANISTER_ID,
                request,
                operation,
                human_readable,
            )
            .await
            .map_err(|err| vec![err])?
        };

        // Topping-up the DEX with asset_0 and asset_1 was successful.
        // Charge the transfer fee.
        let AddLiquidityReply { amount_1, .. } = reply;
        let amount_0 = decode_nat_to_u64(amount_0).unwrap();
        let amount_1 = decode_nat_to_u64(amount_1).unwrap();
        self.move_asset(
            &allowance_0.asset,
            amount_0,
            Party::TreasuryManager,
            Party::External,
        );
        self.move_asset(
            &allowance_1.asset,
            amount_1,
            Party::TreasuryManager,
            Party::External,
        );

        let balances_after = self.get_ledger_balances(operation).await?;
        self.find_discrepency(
            &allowance_0.asset,
            balances_before.0,
            balances_after.0,
            amount_0,
            true,
        );
        self.find_discrepency(
            &allowance_1.asset,
            balances_before.1,
            balances_after.1,
            amount_1,
            true,
        );

        Ok(())
    }

    // TODO refersh balances
    pub async fn deposit_impl(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<ValidatedBalances, Vec<Error>> {
        {
            self.add_manager_balance(&allowance_0.asset, allowance_0.amount_decimals);
            self.add_manager_balance(&allowance_1.asset, allowance_1.amount_decimals);
        }
        let deposit_into_dex_result = self.deposit_into_dex(allowance_0, allowance_1).await;

        let returned_amounts_result = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::new(sns_treasury_manager::Operation::Deposit),
                allowance_0.owner_account,
                allowance_1.owner_account,
            )
            .await;

        self.refresh_balances().await;

        match (deposit_into_dex_result, returned_amounts_result) {
            (Ok(_), Ok(_)) => Ok(self.get_cached_balances()),
            (Ok(_), Err(errs)) => Err(errs),
            (Err(errs), Ok(_)) => Err(errs),
            (Err(errs_1), Err(errs_2)) => {
                let mut errs = errs_1;
                errs.extend(errs_2.into_iter());
                Err(errs)
            }
        }
    }
}
