use crate::{
    log_err,
    state::storage::ConfigState,
    validation::{ValidatedAsset, ValidatedBalances},
    StableAuditTrail, StableBalances,
};
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use std::{cell::RefCell, thread::LocalKey};

pub(crate) mod storage;

pub(crate) struct KongSwapAdaptor<A: AbstractAgent> {
    pub agent: A,
    pub id: Principal,
    pub balances: &'static LocalKey<RefCell<StableBalances>>,
    pub audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn new(
        agent: A,
        id: Principal,
        balances: &'static LocalKey<RefCell<StableBalances>>,
        audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
    ) -> Self {
        KongSwapAdaptor {
            agent,
            id,
            balances,
            audit_trail,
        }
    }

    /// This is safe to call only after the canister has been initialized.
    pub fn get_cached_balances(&self) -> ValidatedBalances {
        self.balances.with_borrow(|balances| {
            let ConfigState::Initialized(balances) = balances.get() else {
                ic_cdk::trap("BUG: Balances should be initialized");
            };
            *balances
        })
    }

    pub fn initialize(&self, init_balances: ValidatedBalances) {
        self.balances.with_borrow_mut(|cell| {
            if let ConfigState::Initialized(balances) = cell.get() {
                log_err(&format!(
                    "Cannot initialize balances: already initialized at timestamp {}",
                    balances.timestamp_ns
                ));
            }

            if let Err(err) = cell.set(ConfigState::Initialized(init_balances)) {
                log_err(&format!("Failed to initialize balances: {:?}", err));
            }
        });
    }

    /// Applies a function to the mutable reference of the balances,
    /// if the canister has been initialized.
    pub fn with_balances_mut<F>(&self, f: F)
    where
        F: FnOnce(&mut ValidatedBalances),
    {
        self.balances.with_borrow_mut(|cell| {
            let ConfigState::Initialized(balances) = cell.get() else {
                return;
            };

            let mut mutable_balances = balances.clone();
            f(&mut mutable_balances);

            if let Err(err) = cell.set(ConfigState::Initialized(mutable_balances)) {
                log_err(&format!("Failed to update balances: {:?}", err));
            }
        })
    }

    pub fn assets(&self) -> (ValidatedAsset, ValidatedAsset) {
        let balances = self.get_cached_balances();
        (balances.asset_0, balances.asset_1)
    }

    pub fn owner_accounts(&self) -> (Account, Account) {
        let balances = self.get_cached_balances();
        (balances.owner_account_0, balances.owner_account_1)
    }

    pub fn ledgers(&self) -> (Principal, Principal) {
        let balances = self.get_cached_balances();
        (
            balances.asset_0.ledger_canister_id(),
            balances.asset_1.ledger_canister_id(),
        )
    }

    pub fn fees(&self) -> (u64, u64) {
        let balances = self.get_cached_balances();
        (
            balances.asset_0.ledger_fee_decimals(),
            balances.asset_1.ledger_fee_decimals(),
        )
    }
}
