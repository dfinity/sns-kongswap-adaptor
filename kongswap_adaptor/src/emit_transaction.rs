use crate::{
    log_err,
    state::{storage::StableTransaction, KongSwapAdaptor},
    StableAuditTrail,
};
use candid::Principal;
use kongswap_adaptor::agent::{AbstractAgent, Request};
use kongswap_adaptor::requests::CommitStateRequest;
use sns_treasury_manager::{TransactionError, TreasuryManagerOperation};
use std::{cell::RefCell, thread::LocalKey};

/// Performs the request call and records the transaction in the audit trail.
async fn emit_transaction<R>(
    audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
    agent: &impl AbstractAgent,
    self_canister_id: Principal,
    canister_id: Principal,
    request: R,
    operation: TreasuryManagerOperation,
    human_readable: String,
) -> Result<R::Ok, TransactionError>
where
    R: Request + Clone,
{
    let call_result = agent
        .call(canister_id, request.clone())
        .await
        .map_err(|error| TransactionError::Call {
            method: request.method().to_string(),
            error: error.to_string(),
            canister_id,
        });

    let (result, function_output) = match call_result {
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

    let transaction = StableTransaction {
        timestamp_ns: ic_cdk::api::time(),
        canister_id,
        result,
        human_readable,
        operation,
    };

    audit_trail.with_borrow_mut(|audit_trail| {
        if let Err(err) = audit_trail.push(&transaction) {
            log_err(&format!(
                "Cannot push transaction to audit trail: {}\ntransaction: {:?}",
                err, transaction
            ));
        }
    });

    // Self-call to ensure that the state has been committed, to prevent state roll back in case
    // of a panic that occurs before the next (meaningfuk) async operation. This is recommended:
    // https://internetcomputer.org/docs/building-apps/security/inter-canister-calls#journaling
    if let Err(err) = agent.call(self_canister_id, CommitStateRequest {}).await {
        log_err(&format!(
            "Failed to commit state after emitting transaction: {}",
            err
        ));
    }

    function_output
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub(crate) async fn emit_transaction<R>(
        &mut self,
        operation: TreasuryManagerOperation,
        canister_id: Principal,
        request: R,
        human_readable: String,
    ) -> Result<R::Ok, TransactionError>
    where
        R: Request + Clone,
    {
        emit_transaction(
            self.audit_trail,
            &self.agent,
            self.id,
            canister_id,
            request,
            operation,
            human_readable,
        )
        .await
    }
}
