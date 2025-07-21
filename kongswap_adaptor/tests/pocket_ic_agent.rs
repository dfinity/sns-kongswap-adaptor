use candid::Principal;
use kongswap_adaptor::agent::{AbstractAgent, Request};
use pocket_ic::nonblocking::PocketIc;
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone)]
pub struct PocketIcAgent {
    pocket_ic: Arc<PocketIc>,
    sender: Principal,
}

impl PocketIcAgent {
    pub fn new(pocket_ic: PocketIc) -> Self {
        let pocket_ic = Arc::new(pocket_ic);
        let sender = Principal::anonymous();
        Self { pocket_ic, sender }
    }

    pub fn pic(&self) -> Arc<PocketIc> {
        self.pocket_ic.clone()
    }

    pub fn with_sender(&mut self, sender: impl Into<Principal>) -> &mut Self {
        self.sender = sender.into();
        self
    }
}

#[derive(Error, Debug)]
pub enum PocketIcCallError {
    #[error("pocket_ic error: {0}")]
    PocketIc(pocket_ic::RejectResponse),
    #[error("canister request could not be encoded: {0}")]
    CandidEncode(candid::Error),
    #[error("canister did not respond with the expected response type: {0}")]
    CandidDecode(candid::Error),
}

impl AbstractAgent for PocketIcAgent {
    type Error = PocketIcCallError;

    async fn call<R: Request>(
        &self,
        canister_id: impl Into<Principal> + Send,
        request: R,
    ) -> Result<R::Response, Self::Error> {
        let canister_id = canister_id.into();

        let request_bytes = request.payload().map_err(PocketIcCallError::CandidEncode)?;

        let response = self
            .pocket_ic
            .update_call(canister_id, self.sender, request.method(), request_bytes)
            .await
            .map_err(PocketIcCallError::PocketIc)?;

        candid::decode_one(response.as_slice()).map_err(PocketIcCallError::CandidDecode)
    }
}
