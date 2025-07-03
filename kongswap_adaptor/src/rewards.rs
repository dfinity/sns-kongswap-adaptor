use crate::{state::KongSwapAdaptor, validation::ValidatedMultiAssetAccounting};
use kongswap_adaptor::agent::AbstractAgent;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub async fn issue_rewards_impl(
        &mut self,
    ) -> Result<ValidatedMultiAssetAccounting, Vec<TransactionError>> {
        // TODO: Ask DEX to send our rewards back.

        let withdraw_accounts = self.owner_accounts();

        let returned_amounts = self
            .return_remaining_assets_to_owner(
                TreasuryManagerOperation::new(sns_treasury_manager::Operation::IssueReward),
                withdraw_accounts[0],
                withdraw_accounts[1],
            )
            .await?;

        Ok(returned_amounts)
    }
}
