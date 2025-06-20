use super::Request;
use candid::{Error, Nat, Principal};
use icrc_ledger_types::icrc2::approve::{ApproveArgs, ApproveError};
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
