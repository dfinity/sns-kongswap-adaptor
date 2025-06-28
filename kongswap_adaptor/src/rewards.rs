use crate::{state::KongSwapAdaptor, validation::ValidatedBalances};
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub async fn issue_rewards_impl(&mut self) -> Result<ValidatedBalances, Vec<TransactionError>> {
        // TODO: Ask DEX to send our rewards back.

        let (withdraw_account_0, withdraw_account_1) = self.owner_accounts();

        let returned_amounts = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::IssueReward,
                withdraw_account_0,
                withdraw_account_1,
            )
            .await?;

        Ok(returned_amounts)
    }
}
