use crate::{state::KongSwapAdaptor, validation::ValidatedBalances};
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::{Operation, TransactionError};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub async fn issue_rewards_impl(&mut self) -> Result<ValidatedBalances, Vec<TransactionError>> {
        let mut context = OperationContext::new(Operation::IssueReward);

        // TODO: Ask DEX to send our rewards back.

        let (withdraw_account_0, withdraw_account_1) = self.owner_accounts();

        let returned_amounts = self
            .return_remaining_assets_to_owner(&mut context, withdraw_account_0, withdraw_account_1)
            .await?;

        Ok(returned_amounts)
    }
}
