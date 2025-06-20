use crate::{
    agent::{CallCanisters, Request},
    KongSwapAdaptor,
};
use candid::Principal;
use sns_treasury_manager::{Transaction, TransactionError, TreasuryManagerOperation};

impl KongSwapAdaptor {
    /// Performs the request call and records the transaction in the audit trail.
    pub async fn emit_transaction<R>(
        &mut self,
        canister_id: Principal,
        request: R,
        phase: TreasuryManagerOperation,
        human_readable: String,
    ) -> Result<R::Ok, TransactionError>
    where
        R: Request + Clone,
    {
        let call_result = self
            .agent
            .call(canister_id, request.clone())
            .await
            .map_err(|error| TransactionError::Call {
                method: request.method().to_string(),
                error: error.to_string(),
                canister_id,
            });

        let (transaction_result, function_output) = match call_result {
            Err(err) => (Err(err.clone()), Err(err)),
            Ok(response) => {
                let res = request
                    .transaction_witness(canister_id, response)
                    .map_err(|err| TransactionError::Backend(err.to_string()));

                match res {
                    Err(err) => (Err(err.clone()), Err(err)),
                    Ok((witness, response)) => (Ok(witness), Ok(response)),
                }
            }
        };

        let transaction = Transaction {
            canister_id,
            result: transaction_result,
            human_readable,
            timestamp_ns: ic_cdk::api::time(),
            treasury_operation_phase: phase,
        };

        self.audit_trail.record_event(transaction);

        function_output
    }
}
