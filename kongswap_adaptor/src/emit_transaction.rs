use crate::{
    log_err,
    state::{storage::StableTransaction, KongSwapAdaptor},
};
use candid::Principal;
use kongswap_adaptor::agent::{AbstractAgent, Request};
use kongswap_adaptor::requests::CommitStateRequest;
use sns_treasury_manager::{Error, TreasuryManagerOperation};

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    /// Performs the request call and records the transaction in the audit trail.
    pub(crate) async fn emit_transaction<R>(
        &mut self,
        operation: TreasuryManagerOperation,
        canister_id: Principal,
        request: R,
        human_readable: String,
    ) -> Result<R::Ok, Error>
    where
        R: Request + Clone,
    {
        let call_result = self
            .agent
            .call(canister_id, request.clone())
            .await
            .map_err(|error| {
                Error::new_call(request.method().to_string(), canister_id, error.to_string())
            });

        let (result, function_output) = match call_result {
            Err(err) => (Err(err.clone()), Err(err)),
            Ok(response) => {
                let res = request
                    .transaction_witness(canister_id, response)
                    .map_err(|err| Error::new_backend(err.to_string()));

                match res {
                    Err(err) => (Err(err.clone()), Err(err)),
                    Ok((witness, response)) => (Ok(witness), Ok(response)),
                }
            }
        };

        let transaction = StableTransaction {
            timestamp_ns: ic_cdk::api::time(),
            canister_id,
            result,
            human_readable,
            operation,
        };

        self.push_audit_trail_transaction(transaction);

        // Self-call to ensure that the state has been committed, to prevent state roll back in case
        // of a panic that occurs before the next (meaningfuk) async operation. This is recommended:
        // https://internetcomputer.org/docs/building-apps/security/inter-canister-calls#journaling
        if let Err(err) = self.agent.call(self.id, CommitStateRequest {}).await {
            log_err(&format!(
                "Failed to commit state after emitting transaction: {}",
                err
            ));
        }

        function_output
    }
}
