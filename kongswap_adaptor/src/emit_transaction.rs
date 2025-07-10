use crate::{
    log_err,
    state::{storage::StableTransaction, KongSwapAdaptor},
    tx_error_codes::TransactionErrorCodes,
    StableAuditTrail,
};
use candid::Principal;
use kongswap_adaptor::agent::{AbstractAgent, Request};
use sns_treasury_manager::{Error, ErrorKind, TreasuryManagerOperation};
use std::{cell::RefCell, thread::LocalKey};

/// Performs the request call and records the transaction in the audit trail.
async fn emit_transaction<R>(
    audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
    agent: &impl AbstractAgent,
    canister_id: Principal,
    request: R,
    treasury_manager_operation: TreasuryManagerOperation,
    human_readable: String,
) -> Result<R::Ok, Error>
where
    R: Request + Clone,
{
    let call_result = agent
        .call(canister_id, request.clone())
        .await
        .map_err(|error| Error {
            code: u64::from(TransactionErrorCodes::CallFailedCode),
            kind: ErrorKind::Call {
                method: request.method().to_string(),
                canister_id,
            },
            message: error.to_string(),
        });

    let (result, function_output) = match call_result {
        Err(err) => (Err(err.clone()), Err(err)),
        Ok(response) => {
            let res = request
                .transaction_witness(canister_id, response)
                .map_err(|err| Error {
                    code: u64::from(TransactionErrorCodes::BackendCode),
                    kind: ErrorKind::Call {
                        method: request.method().to_string(),
                        canister_id,
                    },
                    message: err.to_string(),
                });

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
        treasury_manager_operation,
    };

    audit_trail.with_borrow_mut(|audit_trail| {
        if let Err(err) = audit_trail.push(&transaction) {
            log_err(&format!(
                "Cannot push transaction to audit trail: {}\ntransaction: {:?}",
                err, transaction
            ));
        }
    });

    function_output
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub(crate) async fn emit_transaction<R>(
        &mut self,
        canister_id: Principal,
        request: R,
        treasury_manager_operation: TreasuryManagerOperation,
        human_readable: String,
    ) -> Result<R::Ok, Error>
    where
        R: Request + Clone,
    {
        emit_transaction(
            self.audit_trail,
            &self.agent,
            canister_id,
            request,
            treasury_manager_operation,
            human_readable,
        )
        .await
    }
}
