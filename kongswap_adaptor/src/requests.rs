use crate::agent::Request;
use sns_treasury_manager::{
    AuditTrail, AuditTrailRequest, BalancesRequest, DepositRequest, TreasuryManagerResult,
    WithdrawRequest,
};

impl Request for DepositRequest {
    fn method(&self) -> &'static str {
        "deposit"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = TreasuryManagerResult;

    type Ok = Self::Response;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        _response: Self::Response,
    ) -> Result<(sns_treasury_manager::TransactionWitness, Self::Ok), String> {
        unimplemented!()
    }
}

impl Request for WithdrawRequest {
    fn method(&self) -> &'static str {
        "withdraw"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = TreasuryManagerResult;

    type Ok = Self::Response;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        _response: Self::Response,
    ) -> Result<(sns_treasury_manager::TransactionWitness, Self::Ok), String> {
        unimplemented!()
    }
}

impl Request for BalancesRequest {
    fn method(&self) -> &'static str {
        "balances"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = TreasuryManagerResult;

    type Ok = Self::Response;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        _response: Self::Response,
    ) -> Result<(sns_treasury_manager::TransactionWitness, Self::Ok), String> {
        unimplemented!()
    }
}

impl Request for AuditTrailRequest {
    fn method(&self) -> &'static str {
        "audit_trail"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = AuditTrail;

    type Ok = Self::Response;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        _response: Self::Response,
    ) -> Result<(sns_treasury_manager::TransactionWitness, Self::Ok), String> {
        unimplemented!()
    }
}

pub struct CommitStateRequest {}

impl Request for CommitStateRequest {
    fn method(&self) -> &'static str {
        "commit_state"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        Ok(candid::encode_one(&()).unwrap())
    }

    type Response = ();

    type Ok = ();

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        _response: Self::Response,
    ) -> Result<(sns_treasury_manager::TransactionWitness, Self::Ok), String> {
        Err("CommitStateRequest does not have a transaction witness".to_string())
    }
}
