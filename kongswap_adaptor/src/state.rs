use crate::{
    agent::AbstractAgent,
    validation::{ValidatedAsset, ValidatedBalances},
};
use candid::Principal;
use sns_treasury_manager::AuditTrail;

pub(crate) struct KongSwapAdaptor<A: AbstractAgent> {
    pub agent: A,
    pub kong_backend_canister_id: Principal,
    pub balances: ValidatedBalances,
    pub audit_trail: AuditTrail,
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn new(
        agent: A,
        kong_backend_canister_id: Principal,
        asset_0: ValidatedAsset,
        asset_1: ValidatedAsset,
    ) -> Self {
        let audit_trail = AuditTrail::new();

        let balances = ValidatedBalances {
            asset_0,
            asset_1,
            balance_0_decimals: 0,
            balance_1_decimals: 0,
            timestamp_ns: 0,
        };

        KongSwapAdaptor {
            agent,
            kong_backend_canister_id,
            audit_trail,
            balances,
        }
    }
}
