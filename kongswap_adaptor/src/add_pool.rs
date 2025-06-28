use candid::Nat;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

use crate::{
    accounting::{self, Party},
    agent::AbstractAgent,
    deposit::DEPOSIT_LEDGER_FEES_PER_TOKEN,
    emit_transaction::emit_transaction,
    kong_types::{AddPoolArgs, AddPoolReply},
    state::KongSwapAdaptor,
    validation::{saturating_sub, ValidatedAllowance, ValidatedBalances},
};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub(crate) async fn try_add_pool(
        &mut self,
        allowance_0: &ValidatedAllowance,
        allowance_1: &ValidatedAllowance,
    ) -> Result<ValidatedBalances, TransactionError> {
        let owner_account_0 = allowance_0.owner_account;
        let ledger_0 = allowance_0.asset.ledger_canister_id();
        let fee_0 =
            Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_0.asset.ledger_fee_decimals();
        let amount_0 = saturating_sub(Nat::from(allowance_0.amount_decimals), fee_0.clone());

        let owner_account_1 = allowance_1.owner_account;
        let ledger_1 = allowance_1.asset.ledger_canister_id();
        let fee_1 =
            Nat::from(DEPOSIT_LEDGER_FEES_PER_TOKEN) * allowance_1.asset.ledger_fee_decimals();
        let amount_1 = saturating_sub(Nat::from(allowance_1.amount_decimals), fee_1.clone());

        let token_0 = format!("IC.{}", ledger_0);
        let token_1 = format!("IC.{}", ledger_1);

        let result = emit_transaction(
            &mut self.audit_trail,
            &self.agent,
            self.kong_backend_canister_id,
            AddPoolArgs {
                token_0: token_0.clone(),
                amount_0: amount_0.clone(),
                token_1: token_1.clone(),
                amount_1: amount_1.clone(),

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

        return match result {
            // All used up, since the pool is brand new.
            Ok(AddPoolReply {
                symbol_0,
                address_0,
                amount_0,
                symbol_1,
                amount_1,
                address_1,
                ..
            }) => {
                // ------ Book keeping ------
                let entries_0 = accounting::create_ledger_entries(
                    Party::Sns,
                    Party::External,
                    amount_0.clone(),
                    fee_0.clone(),
                )?;
                self.accounting
                    .post_asset_transaction(&allowance_0.asset, &entries_0);

                let entries_1 = accounting::create_ledger_entries(
                    Party::Sns,
                    Party::External,
                    amount_1.clone(),
                    fee_1.clone(),
                )?;
                self.accounting
                    .post_asset_transaction(&allowance_1.asset, &entries_1);

                // -------------------------

                self.reply_params_to_result(
                    symbol_0,
                    address_0,
                    amount_0,
                    owner_account_0,
                    symbol_1,
                    amount_1,
                    address_1,
                    owner_account_1,
                )
            }

            Err(err) => Err(err),
        };
    }
}
