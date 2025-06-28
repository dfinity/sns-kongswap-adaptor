use crate::{
    validation::{ValidatedAsset, ValidatedBalances},
    StableAuditTrail,
};
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use std::{cell::RefCell, thread::LocalKey};

pub(crate) mod storage;

pub(crate) struct KongSwapAdaptor<A: AbstractAgent> {
    pub agent: A,
    pub balances: &'static LocalKey<RefCell<Option<ValidatedBalances>>>,
    pub audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn new(
        agent: A,
        balances: &'static LocalKey<RefCell<Option<ValidatedBalances>>>,
        audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
    ) -> Self {
        KongSwapAdaptor {
            agent,
            balances,
            audit_trail,
        }
    }

    pub fn assets(&self) -> (ValidatedAsset, ValidatedAsset) {
        self.balances.with_borrow(|balances| {
            let balances = balances.as_ref().expect("Balances should be initialized");
            (balances.asset_0, balances.asset_1)
        })
    }

    pub fn owner_accounts(&self) -> (Account, Account) {
        self.balances.with_borrow(|balances| {
            let balances = balances.as_ref().expect("Balances should be initialized");
            (balances.owner_account_0, balances.owner_account_1)
        })
    }

    pub fn ledgers(&self) -> (Principal, Principal) {
        self.balances.with_borrow(|balances| {
            let balances = balances.as_ref().expect("Balances should be initialized");
            (
                balances.asset_0.ledger_canister_id(),
                balances.asset_1.ledger_canister_id(),
            )
        })
    }

    pub fn fees(&self) -> (u64, u64) {
        self.balances.with_borrow(|balances| {
            let balances = balances.as_ref().expect("Balances should be initialized");
            (
                balances.asset_0.ledger_fee_decimals(),
                balances.asset_1.ledger_fee_decimals(),
            )
        })
    }
}
