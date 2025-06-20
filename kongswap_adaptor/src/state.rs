use candid::Principal;
// use ic_stable_structures::{memory_manager::MemoryManager, Cell, DefaultMemoryImpl};
use crate::{agent::ic_cdk_agent::CdkAgent, validation::ValidatedAsset};
use sns_treasury_manager::{Asset, AuditTrail};

pub(crate) struct KongSwapAdaptor {
    pub agent: CdkAgent,

    pub kong_backend_canister_id: Principal,

    pub token_0: ValidatedAsset,
    pub token_1: ValidatedAsset,

    pub balance_0_decimals: u64,
    pub balance_1_decimals: u64,

    pub audit_trail: AuditTrail,
}

impl KongSwapAdaptor {
    pub fn new(
        kong_backend_canister_id: Principal,
        token_0: ValidatedAsset,
        token_1: ValidatedAsset,
    ) -> Self {
        let audit_trail = AuditTrail::new();

        let agent = CdkAgent::new();

        KongSwapAdaptor {
            agent,
            kong_backend_canister_id,
            token_0,
            token_1,
            audit_trail,
            balance_0_decimals: 0,
            balance_1_decimals: 0,
        }
    }
}
