use crate::{
    accounting::MultiAssetAccounting,
    agent::AbstractAgent,
    validation::{ValidatedAsset, ValidatedBalances},
};
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use sns_treasury_manager::AuditTrail;

pub(crate) struct KongSwapAdaptor<A: AbstractAgent> {
    pub agent: A,
    pub kong_backend_canister_id: Principal,
    pub balances: ValidatedBalances,
    pub audit_trail: AuditTrail,
    pub accounting: MultiAssetAccounting,
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn new(
        agent: A,
        kong_backend_canister_id: Principal,
        asset_0: ValidatedAsset,
        asset_1: ValidatedAsset,
        owner_account_0: Account,
        owner_account_1: Account,
    ) -> Self {
        let audit_trail = AuditTrail::new();

        let balances = ValidatedBalances {
            timestamp_ns: 0,
            balance_0_decimals: 0,
            balance_1_decimals: 0,
            asset_0,
            asset_1,
            owner_account_0,
            owner_account_1,
        };

        let accounting = MultiAssetAccounting::new([asset_0, asset_1].to_vec());

        KongSwapAdaptor {
            agent,
            kong_backend_canister_id,
            audit_trail,
            balances,
            accounting,
        }
    }
}
