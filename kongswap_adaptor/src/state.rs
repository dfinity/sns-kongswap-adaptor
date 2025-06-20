use candid::Principal;
// use ic_stable_structures::{memory_manager::MemoryManager, Cell, DefaultMemoryImpl};
use crate::{
    agent::ic_cdk_agent::CdkAgent,
    validation::{ValidatedAsset, ValidatedBalances},
};
use sns_treasury_manager::AuditTrail;

pub(crate) struct KongSwapAdaptor {
    pub agent: CdkAgent,

    pub kong_backend_canister_id: Principal,

    pub balances: ValidatedBalances,

    pub audit_trail: AuditTrail,
}

impl KongSwapAdaptor {
    pub fn new(
        kong_backend_canister_id: Principal,
        asset_0: ValidatedAsset,
        asset_1: ValidatedAsset,
    ) -> Self {
        let audit_trail = AuditTrail::new();

        let agent = CdkAgent::new();

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
