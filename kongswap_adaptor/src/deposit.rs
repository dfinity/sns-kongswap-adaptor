use crate::{
    kong_api::reply_params_to_result,
    kong_types::{
        AddLiquidityAmountsArgs, AddLiquidityAmountsReply, AddLiquidityArgs, AddLiquidityReply,
        AddPoolArgs, AddPoolReply,
    },
    validation::{ValidatedAllowance, ValidatedAsset},
    KongSwapAdaptor,
};
use candid::Nat;
use icrc_ledger_types::{icrc1::account::Account, icrc2::approve::ApproveArgs};
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};
use std::collections::BTreeMap;

/// How many ledger transaction that incur fees are required for a deposit operation (per token).
/// This is an implementation detail of KongSwap and ICRC1 ledgers.
const DEPOSIT_LEDGER_FEES_PER_TOKEN: u64 = 2;

impl KongSwapAdaptor {
    pub async fn deposit_impl(
        &mut self,
        allowance_0: ValidatedAllowance,
        allowance_1: ValidatedAllowance,
    ) -> Result<BTreeMap<ValidatedAsset, u64>, TransactionError> {
        let phase = TreasuryManagerOperation::Deposit;

        // Additional validation.
        {
            let ledger_0 = allowance_0.asset.ledger_canister_id();
            if ledger_0 != self.token_0.ledger_canister_id() {
                return Err(TransactionError::Precondition(format!(
                    "KongSwapAdaptor only supports {} as token_0 (got ledger {}).",
                    self.token_0.symbol(),
                    ledger_0
                )));
            }
        }

        // Step 1. Set up the allowances for the KongSwapBackend canister.
        for ValidatedAllowance {
            asset,
            amount_decimals,
            expected_ledger_fee_decimals,
        } in [&allowance_0, &allowance_1]
        {
            let human_readable = format!(
                "Calling ICRC2 approve to set KongSwapBackend as spender for {}.",
                asset.symbol()
            );
            let canister_id = asset.ledger_canister_id();
            let fee_decimals = Nat::from(*expected_ledger_fee_decimals);
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
            self.emit_transaction(canister_id, request, phase, human_readable)
                .await?;
        }

        let ledger_0 = allowance_0.asset.ledger_canister_id();
        let ledger_1 = allowance_1.asset.ledger_canister_id();

        // Adjust the amounts to reflect that `DEPOSIT_LEDGER_FEES_PER_TOKEN` transactions
        // (per token) are required for adding liquidity.
        //
        // The call to `validate_allowances` above ensures that the amounts are still
        // sufficiently large.
        let amount_0 = allowance_0.amount_decimals
            - Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_0.expected_ledger_fee_decimals;
        let amount_1 = allowance_1.amount_decimals
            - Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_1.expected_ledger_fee_decimals;

        // Step 2. Ensure the tokens are registered with the DEX.
        // Notes on why we first add SNS and then ICP:
        // - KongSwap starts indexing tokens from 1.
        // - The ICP token is assumed to have index 2.
        self.maybe_add_token(ledger_0, phase).await?;
        self.maybe_add_token(ledger_1, phase).await?;

        // Step 3. Ensure the pool exists.

        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        let original_amount_1 = amount_1.clone();

        let result = self
            .emit_transaction(
                self.kong_backend_canister_id,
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
                TreasuryManagerOperation::Deposit,
                "Calling KongSwapBackend.add_pool to add a new pool.".to_string(),
            )
            .await;

        let pool_already_exists = { format!("Pool {} already exists", self.lp_token()) };

        match result {
            // All used up, since the pool is brand new.
            Ok(AddPoolReply {
                status,
                symbol_0,
                address_0,
                amount_0,
                symbol_1,
                amount_1,
                address_1,
                ..
            }) => {
                return reply_params_to_result(
                    "add_pool", status, symbol_0, address_0, amount_0, symbol_1, amount_1,
                    address_1,
                );
            }

            // An already-existing pool does not preclude a top-up  =>  Keep going.
            Err(TransactionError::Backend(err)) if *err == pool_already_exists => (),

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

            self.emit_transaction(
                self.kong_backend_canister_id,
                request,
                phase,
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

            self.emit_transaction(
                self.kong_backend_canister_id,
                request,
                phase,
                human_readable,
            )
            .await?
        };

        let AddLiquidityReply {
            status,
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

        reply_params_to_result(
            "add_liquidity",
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
