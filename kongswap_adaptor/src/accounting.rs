use std::collections::BTreeMap;

use candid::CandidType;
use serde::Deserialize;
use sns_treasury_manager::Balance;

use crate::validation::ValidatedAsset;

// aterga icrc_account1, 10 icp, Treasuryowner
// icp => TreasuryOwner => Balance { 10, account: Some(aterga) }
// icp => External => Balance {0, account: None}
// icp => FeeCollector => Balance { 0, account: None}
// aterga deposits 5 icp's
// icp => TreasuryOwner => Balance { 5, account: Some(aterga) }
// icp => External => Balance {5 - fee, account: None}
// icp => FeeCollector => Balance { fee, account: None}

// aterga icrc_account1, 10 icp, Treasuryowner
// icp => btreemap!{}
// sns => btreemap!{}
#[derive(CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalancesForAsset {
    pub treasury_owner: Balance,
    pub treasury_manager: Balance,
    pub external: Balance,
    pub fee_collector: Balance,
}

#[derive(CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalances {
    pub timestamp_ns: u64,
    pub asset_to_balances: BTreeMap<ValidatedAsset, ValidatedBalancesForAsset>,
}

impl ValidatedBalances {
    pub(crate) fn new() -> Self {
        todo!()
    }
    pub(crate) fn refresh_asset(&mut self) {}

    pub(crate) fn refresh_party_balances(&mut self) {}
}
