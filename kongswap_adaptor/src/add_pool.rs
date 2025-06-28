use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

use crate::{
    agent::AbstractAgent,
    emit_transaction::emit_transaction,
    kong_types::{AddPoolArgs, AddPoolReply},
    state::KongSwapAdaptor,
    validation::ValidatedBalances,
};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub(crate) async fn try_add_pool(
        &mut self,
        amount_0: &Nat,
        amount_1: &Nat,
        ledger_0: Principal,
        ledger_1: Principal,
        owner_account_0: Account,
        owner_account_1: Account,
    ) -> Result<ValidatedBalances, TransactionError> {
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
            }) => self.reply_params_to_result(
                symbol_0,
                address_0,
                amount_0,
                owner_account_0,
                symbol_1,
                amount_1,
                address_1,
                owner_account_1,
            ),

            Err(err) => Err(err),
        };
    }
}
