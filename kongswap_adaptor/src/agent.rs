use candid::{CandidType, Principal};
use serde::de::DeserializeOwned;
use sns_treasury_manager::TransactionWitness;
use std::{error::Error, fmt::Display, future::Future};

pub mod ic_cdk_agent;
pub mod icrc_requests;

pub trait Request: Send {
    fn method(&self) -> &'static str;
    fn payload(&self) -> Result<Vec<u8>, candid::Error>;

    type Response: CandidType + DeserializeOwned + Send;

    /// The type representing the successful response from the canister.
    ///
    /// Either the same, or a sub-structure of `Response`.
    type Ok: CandidType + DeserializeOwned + Send;

    fn transaction_witness(
        &self,
        canister_id: Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String>;
}

pub trait AbstractAgent: Clone + Send + Sync {
    type Error: Display + Send + Error + 'static;

    fn call<R: Request>(
        &self,
        canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> impl Future<Output = Result<R::Response, Self::Error>> + Send;
}
