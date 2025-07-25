use crate::state::KongSwapAdaptor;
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::Error;

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub async fn issue_rewards_impl(
        &mut self,
        context: &mut OperationContext,
    ) -> Result<(), Vec<Error>> {
        // TODO: Ask DEX to send our rewards back.

        let (withdraw_account_0, withdraw_account_1) = self.owner_accounts();

        self.return_remaining_assets_to_owner(context, withdraw_account_0, withdraw_account_1)
            .await
    }
}
