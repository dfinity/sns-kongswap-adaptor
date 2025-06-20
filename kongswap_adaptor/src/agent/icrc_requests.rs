//! This module contains implementations of the `Request` trait for some ICRC-1 and ICRC-2
//! functions used in the KongSwap Adaptor canister. See https://github.com/dfinity/ICRC-1

use super::Request;
use candid::{CandidType, Error, Nat, Principal};
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue;
use icrc_ledger_types::icrc2::approve::{ApproveArgs, ApproveError};
use serde::Serialize;
use sns_treasury_manager::{TransactionWitness, Transfer};

impl Request for ApproveArgs {
    fn method(&self) -> &'static str {
        "icrc2_approve"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, Error> {
        candid::encode_one(self)
    }

    type Response = Result<Nat, ApproveError>;

    type Ok = Nat;

    fn transaction_witness(
        &self,
        canister_id: Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let block_index = response.map_err(|err| err.to_string())?;

        let ledger_canister_id = canister_id.to_string();
        let amount_decimals = self.amount.clone();

        let witness = TransactionWitness::Ledger(vec![Transfer {
            ledger_canister_id,
            amount_decimals,
            block_index: block_index.clone(),
        }]);

        Ok((witness, block_index))
    }
}

#[derive(CandidType, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Icrc1MetadataRequest {}

impl Request for Icrc1MetadataRequest {
    fn method(&self) -> &'static str {
        "icrc1_metadata"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, Error> {
        candid::encode_one(())
    }

    type Response = Vec<(String, MetadataValue)>;

    type Ok = Self::Response;

    fn transaction_witness(
        &self,
        _canister_id: Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let response_str = format!("{:?}", response);
        Ok((TransactionWitness::NonLedger(response_str), response))
    }
}
