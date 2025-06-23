use candid::{CandidType, Nat};
use serde::{Deserialize, Serialize};
use sns_treasury_manager::{TransactionWitness, Transfer};

use crate::agent::Request;

const E8: u64 = 100_000_000; // 10^8, used for converting LP balances to decimals

// ----------------- begin:add_liquidity_amounts -----------------
pub fn kong_lp_balance_to_decimals(lp_balance: f64) -> Result<Nat, String> {
    // Check that lp_balance is valid before conversion
    if !lp_balance.is_finite() || lp_balance < 0.0 {
        return Err("Invalid LP balance value".to_string());
    }

    // Calculate with overflow checking
    let e8_value = E8 as f64;
    let result_f64 = lp_balance * e8_value;

    // Ensure the result fits in u64 range
    if result_f64 > u64::MAX as f64 {
        return Err("LP balance conversion exceeds u64 maximum".to_string());
    }

    // Convert to Nat (safe because we've checked the bounds)
    Ok(Nat::from(result_f64.round() as u64))
}

#[derive(CandidType, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddLiquidityAmountsArgs {
    pub token_0: String,
    pub amount: Nat,
    pub token_1: String,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddLiquidityAmountsReply {
    pub symbol: String,
    pub chain_0: String,
    pub address_0: String,
    pub symbol_0: String,
    pub amount_0: Nat,
    pub fee_0: Nat,
    pub chain_1: String,
    pub address_1: String,
    pub symbol_1: String,
    pub amount_1: Nat,
    pub fee_1: Nat,
    pub add_lp_token_amount: Nat,
}

impl Request for AddLiquidityAmountsArgs {
    fn method(&self) -> &'static str {
        "add_liquidity_amounts"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        let Self {
            token_0,
            amount,
            token_1,
        } = self.clone();

        candid::encode_args((token_0, amount, token_1))
    }

    type Response = Result<AddLiquidityAmountsReply, String>;

    type Ok = AddLiquidityAmountsReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", reply));

        Ok((witness, reply))
    }
}
// ----------------- end:add_liquidity_amounts -----------------

// ----------------- begin:add_liquidity -----------------
impl Request for AddLiquidityArgs {
    fn method(&self) -> &'static str {
        "add_liquidity"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = Result<AddLiquidityReply, String>;

    type Ok = AddLiquidityReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        let transfers = reply.transfer_ids.iter().map(Transfer::from).collect();

        let witness = TransactionWitness::Ledger(transfers);

        Ok((witness, reply))
    }
}

#[derive(CandidType, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxId {
    BlockIndex(Nat),
    TransactionHash(String),
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddLiquidityArgs {
    pub token_0: String,
    pub amount_0: Nat,
    pub tx_id_0: Option<TxId>,
    pub token_1: String,
    pub amount_1: Nat,
    pub tx_id_1: Option<TxId>,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddLiquidityReply {
    pub tx_id: u64,
    pub request_id: u64,
    pub status: String,
    pub symbol: String,
    pub chain_0: String,
    pub address_0: String,
    pub symbol_0: String,
    pub amount_0: Nat,
    pub chain_1: String,
    pub address_1: String,
    pub symbol_1: String,
    pub amount_1: Nat,
    pub add_lp_token_amount: Nat,
    pub transfer_ids: Vec<TransferIdReply>,
    pub claim_ids: Vec<u64>,
    pub ts: u64,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct TransferIdReply {
    pub transfer_id: u64,
    pub transfer: TransferReply,
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
// ----------------- end:add_liquidity -----------------

// ----------------- begin:add_token -----------------
impl Request for AddTokenArgs {
    fn method(&self) -> &'static str {
        "add_token"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = Result<AddTokenReply, String>;

    type Ok = AddTokenReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", self));

        Ok((witness, reply))
    }
}

// Arguments for adding a token.
#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddTokenArgs {
    pub token: String,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub enum AddTokenReply {
    IC(ICReply),
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct ICReply {
    pub token_id: u32,
    pub chain: String,
    pub canister_id: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub fee: Nat,
    pub icrc1: bool,
    pub icrc2: bool,
    pub icrc3: bool,
    pub is_removed: bool,
}
// ----------------- end:add_token -----------------

// ----------------- begin:update_token -----------------
#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTokenArgs {
    pub token: String,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub enum UpdateTokenReply {
    IC(ICReply),
}

impl Request for UpdateTokenArgs {
    fn method(&self) -> &'static str {
        "update_token"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = Result<UpdateTokenReply, String>;

    type Ok = UpdateTokenReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", self));

        Ok((witness, reply))
    }
}
// ----------------- end:update_token -----------------

// ----------------- begin:add_pool -----------------
impl Request for AddPoolArgs {
    fn method(&self) -> &'static str {
        "add_pool"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = Result<AddPoolReply, String>;

    type Ok = AddPoolReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        let transfers = reply.transfer_ids.iter().map(Transfer::from).collect();

        let witness = TransactionWitness::Ledger(transfers);

        Ok((witness, reply))
    }
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddPoolReply {
    pub tx_id: u64,
    pub pool_id: u32,
    pub request_id: u64,
    pub status: String,
    pub name: String,
    pub symbol: String,
    pub chain_0: String,
    pub address_0: String,
    pub symbol_0: String,
    pub amount_0: Nat,
    pub balance_0: Nat,
    pub chain_1: String,
    pub address_1: String,
    pub symbol_1: String,
    pub amount_1: Nat,
    pub balance_1: Nat,
    pub lp_fee_bps: u8,
    pub lp_token_symbol: String,
    pub add_lp_token_amount: Nat,
    pub transfer_ids: Vec<TransferIdReply>,
    pub claim_ids: Vec<u64>,
    pub is_removed: bool,
    pub ts: u64,
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
        }
    }
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct AddPoolArgs {
    pub token_0: String,
    pub amount_0: Nat,
    pub tx_id_0: Option<TxId>,
    pub token_1: String,
    pub amount_1: Nat,
    pub tx_id_1: Option<TxId>,
    pub lp_fee_bps: Option<u8>,
}
// ----------------- end:add_pool -----------------

// ----------------- begin:tokens -----------------
impl Request for TokensArgs {
    fn method(&self) -> &'static str {
        "tokens"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self.symbol.clone())
    }

    type Response = Result<Vec<TokensReply>, String>;

    type Ok = Vec<TokensReply>;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", reply));

        Ok((witness, reply))
    }
}

struct TokensArgs {
    pub symbol: Option<String>,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub enum TokensReply {
    LP(LPReply),
    IC(ICReply),
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct LPReply {
    pub token_id: u32,
    pub chain: String,
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub pool_id_of: u32,
    pub decimals: u8,
    pub fee: Nat,
    pub total_supply: Nat,
    pub is_removed: bool,
}
// ----------------- end:tokens -----------------

// ----------------- begin:tokens -----------------
impl Request for PoolsArgs {
    fn method(&self) -> &'static str {
        "pools"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self.symbol.clone())
    }

    type Response = Result<Vec<PoolReply>, String>;

    type Ok = Vec<PoolReply>;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", reply));

        Ok((witness, reply))
    }
}

struct PoolsArgs {
    pub symbol: Option<String>,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct PoolReply {
    pub pool_id: u32,
    pub name: String,
    pub symbol: String,
    pub chain_0: String,
    pub symbol_0: String,
    pub address_0: String,
    pub balance_0: Nat,
    pub lp_fee_0: Nat,
    pub chain_1: String,
    pub symbol_1: String,
    pub address_1: String,
    pub balance_1: Nat,
    pub lp_fee_1: Nat,
    pub price: f64,
    pub lp_fee_bps: u8,
    pub lp_token_symbol: String,
    pub is_removed: bool,
}
// ----------------- end:tokens -----------------

// ----------------- begin:remove_liquidity_amounts -----------------
impl Request for RemoveLiquidityAmountsArgs {
    fn method(&self) -> &'static str {
        "remove_liquidity_amounts"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        let Self {
            token_0,
            token_1,
            remove_lp_token_amount,
        } = self;

        candid::encode_args((token_0, token_1, remove_lp_token_amount))
    }

    type Response = Result<RemoveLiquidityAmountsReply, String>;

    type Ok = RemoveLiquidityAmountsReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        // TODO: Use serde_json::to_string
        let witness = TransactionWitness::NonLedger(format!("{:?}", reply));

        Ok((witness, reply))
    }
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct RemoveLiquidityAmountsArgs {
    pub token_0: String,
    pub token_1: String,
    pub remove_lp_token_amount: Nat,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct RemoveLiquidityAmountsReply {
    pub symbol: String,
    pub chain_0: String,
    pub address_0: String,
    pub symbol_0: String,
    pub amount_0: Nat,
    pub lp_fee_0: Nat,
    pub chain_1: String,
    pub address_1: String,
    pub symbol_1: String,
    pub amount_1: Nat,
    pub lp_fee_1: Nat,
    pub remove_lp_token_amount: Nat,
}
// ----------------- end:remove_liquidity_amounts -----------------

// ----------------- begin:liquidity_amounts -----------------
impl Request for RemoveLiquidityArgs {
    fn method(&self) -> &'static str {
        "remove_liquidity"
    }

    fn update(&self) -> bool {
        true
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self)
    }

    type Response = Result<RemoveLiquidityReply, String>;

    type Ok = RemoveLiquidityReply;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let reply = response?;

        let transfers = reply.transfer_ids.iter().map(Transfer::from).collect();

        let witness = TransactionWitness::Ledger(transfers);

        Ok((witness, reply))
    }
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct RemoveLiquidityReply {
    pub tx_id: u64,
    pub request_id: u64,
    pub status: String,
    pub symbol: String,
    pub chain_0: String,
    pub address_0: String,
    pub symbol_0: String,
    pub amount_0: Nat,
    pub lp_fee_0: Nat,
    pub chain_1: String,
    pub address_1: String,
    pub symbol_1: String,
    pub amount_1: Nat,
    pub lp_fee_1: Nat,
    pub remove_lp_token_amount: Nat,
    pub transfer_ids: Vec<TransferIdReply>,
    pub claim_ids: Vec<u64>,
    pub ts: u64,
}

#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct RemoveLiquidityArgs {
    pub token_0: String,
    pub token_1: String,
    pub remove_lp_token_amount: Nat,
}
// ----------------- end:liquidity_amounts -----------------

// ----------------- begin:user_balances -----------------
impl Request for UserBalancesArgs {
    fn method(&self) -> &'static str {
        "user_balances"
    }

    fn update(&self) -> bool {
        false
    }

    fn payload(&self) -> Result<Vec<u8>, candid::Error> {
        candid::encode_one(self.principal_id.clone())
    }

    type Response = Result<Vec<UserBalancesReply>, String>;

    type Ok = Vec<UserBalancesReply>;

    fn transaction_witness(
        &self,
        _canister_id: candid::Principal,
        response: Self::Response,
    ) -> Result<(TransactionWitness, Self::Ok), String> {
        let replies = response?;

        let witnesses = replies
            .iter()
            .map(|UserBalancesReply::LP(user_balance_lp_reply)| {
                // TODO: Use serde_json::to_string
                format!("{:?}", user_balance_lp_reply)
            })
            .collect::<Vec<_>>();

        let witness = TransactionWitness::NonLedger(witnesses.join(", "));

        Ok((witness, replies))
    }
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct UserBalancesArgs {
    pub principal_id: String,
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub enum UserBalancesReply {
    LP(UserBalanceLPReply),
}

#[derive(CandidType, Clone, Debug, Serialize, Deserialize)]
pub struct UserBalanceLPReply {
    pub symbol: String,
    pub name: String,
    pub lp_token_id: u64,
    pub balance: f64,
    pub usd_balance: f64,
    pub chain_0: String,
    pub symbol_0: String,
    pub address_0: String,
    pub amount_0: f64,
    pub usd_amount_0: f64,
    pub chain_1: String,
    pub symbol_1: String,
    pub address_1: String,
    pub amount_1: f64,
    pub usd_amount_1: f64,
    pub ts: u64,
}

// ----------------- end:user_balances -----------------
