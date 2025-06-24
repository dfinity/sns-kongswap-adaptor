use crate::{agent::AbstractAgent, state::KongSwapAdaptor, validation::ValidatedBalances};
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub async fn issue_rewards_impl(&mut self) -> Result<ValidatedBalances, Vec<TransactionError>> {
        // TODO: Ask DEX to send our rewards back.

        let returned_amounts = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::IssueReward,
                self.balances.owner_account_0,
                self.balances.owner_account_1,
            )
            .await?;

        Ok(returned_amounts)
    }
}
