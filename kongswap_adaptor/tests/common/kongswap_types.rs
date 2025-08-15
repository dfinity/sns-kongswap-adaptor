use candid::{CandidType, Nat};
use kongswap_adaptor::{agent::Request, audit::serialize_reply};
use serde::{Deserialize, Serialize};
use sns_treasury_manager::{TransactionWitness, Transfer};

// The next functions and structures are used only for integration testing.
#[derive(CandidType, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxId {
    BlockIndex(Nat),
    TransactionHash(String),
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub enum TransferReply {
    IC(ICTransferReply),
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct ICTransferReply {
    pub chain: String,
    pub symbol: String,
    pub is_send: bool, // from user's perspective. so if is_send is true, it means the user is sending the token
    pub amount: Nat,
    pub canister_id: String,
    pub block_index: Nat,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct TransferIdReply {
    pub transfer_id: u64,
    pub transfer: TransferReply,
}

fn empty_string() -> String {
    String::new()
}
#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct SwapTxReply {
    pub pool_symbol: String,
    pub pay_chain: String,
    #[serde(default = "empty_string")]
    pub pay_address: String,
    pub pay_symbol: String,
    pub pay_amount: Nat,
    pub receive_chain: String,
    #[serde(default = "empty_string")]
    pub receive_address: String,
    pub receive_symbol: String,
    pub receive_amount: Nat, // including fees
    pub price: f64,
    pub lp_fee: Nat,  // will be in receive_symbol
    pub gas_fee: Nat, // will be in receive_symbol
    pub ts: u64,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct SwapReply {
    pub tx_id: u64,
    pub request_id: u64,
    pub status: String,
    pub pay_chain: String,
    #[serde(default = "empty_string")]
    pub pay_address: String,
    pub pay_symbol: String,
    pub pay_amount: Nat,
    pub receive_chain: String,
    #[serde(default = "empty_string")]
    pub receive_address: String,
    pub receive_symbol: String,
    pub receive_amount: Nat,
    pub mid_price: f64,
    pub price: f64,
    pub slippage: f64,
    pub txs: Vec<SwapTxReply>,
    pub transfer_ids: Vec<TransferIdReply>,
    pub claim_ids: Vec<u64>,
    pub ts: u64,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct SwapArgs {
    pub pay_token: String,
    pub pay_amount: Nat,
    pub pay_tx_id: Option<TxId>,
    pub receive_token: String,
    pub receive_amount: Option<Nat>,
    pub receive_address: Option<String>,
    pub max_slippage: Option<f64>,
    pub referred_by: Option<String>,
}

impl Request for SwapArgs {
    fn method(&self) -> &'static str {
        "swap"
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(&self)
    }

    type Response = Result<SwapReply, String>;

    type Ok = SwapReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        if reply.status != "Success" {
            return Err(format!("Failed to claim: {}", serialize_reply(&reply)));
        }

        let transfers = reply.transfer_ids.iter().map(Transfer::from).collect();

        let witness = TransactionWitness::Ledger(transfers);

        Ok((witness, reply))
    }
}

impl From<&TransferIdReply> for Transfer {
    fn from(transfer_id_reply: &TransferIdReply) -> Self {
        let TransferIdReply {
            transfer_id: _,
            transfer:
                TransferReply::IC(ICTransferReply {
                    amount,
                    canister_id,
                    block_index,
                    ..
                }),
        } = transfer_id_reply;

        let ledger_canister_id = canister_id.clone();
        let amount_deimals = amount.clone();
        let block_index = block_index.clone();

        Self {
            ledger_canister_id,
            amount_decimals: amount_deimals,
            block_index,
            sender: None,
            receiver: None,
        }
    }
}
