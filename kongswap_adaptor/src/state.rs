use crate::{
    balances::{Party, ValidatedBalances},
    balances::{Party, ValidatedBalances},
    log_err, log_err,
    state::storage::ConfigState,
    state::storage::ConfigState,
    validation::ValidatedAsset,
    validation::ValidatedAsset,
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
            balances.clone()
        })
    }

    pub fn initialize(
        &self,
        asset_0: ValidatedAsset,
        asset_1: ValidatedAsset,
        owner_account_0: Account,
        owner_account_1: Account,
    ) {
        self.balances.with_borrow_mut(|cell| {
            if let ConfigState::Initialized(balances) = cell.get() {
                log_err(&format!(
                    "Cannot initialize balances: already initialized at timestamp {}",
                    balances.timestamp_ns
                ));
            }

            let validated_balances =
                ValidatedBalances::new(asset_0, asset_1, owner_account_0, owner_account_1);

            if let Err(err) = cell.set(ConfigState::Initialized(validated_balances)) {
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
        let validated_balances = self.get_cached_balances();
        (validated_balances.asset_0, validated_balances.asset_1)
    }

    pub fn owner_accounts(&self) -> (Account, Account) {
        let validated_balances = self.get_cached_balances();
        (
            validated_balances.asset_0_balance.treasury_owner.account,
            validated_balances.asset_1_balance.treasury_owner.account,
        )
    }

    pub fn ledgers(&self) -> (Principal, Principal) {
        let balances = self.get_cached_balances();
        (
            balances.asset_0.ledger_canister_id(),
            balances.asset_1.ledger_canister_id(),
        )
    }

    pub fn charge_fee(&mut self, asset: &ValidatedAsset) {
        self.with_balances_mut(|validated_balances| validated_balances.charge_fee(asset));
    }

    pub fn move_asset(&mut self, asset: &ValidatedAsset, amount: u64, from: Party, to: Party) {
        self.with_balances_mut(|validated_balances| {
            validated_balances.move_asset(asset, from, to, amount)
        });
    }

    pub fn add_manager_balance(&mut self, asset: &ValidatedAsset, amount: u64) {
        self.with_balances_mut(|validated_balances| {
            validated_balances.add_manager_balance(asset, amount)
        });
    }

    pub fn find_discrepency(
        &mut self,
        asset: &ValidatedAsset,
        balance_before: u64,
        balance_after: u64,
        transferred_amount: u64,
        is_deposit: bool,
    ) {
        self.with_balances_mut(|validated_balances| {
            if is_deposit {
                validated_balances.find_deposit_discrepency(
                    asset,
                    balance_before,
                    balance_after,
                    transferred_amount,
                );
            } else {
                validated_balances.find_withdraw_discrepency(
                    asset,
                    balance_before,
                    balance_after,
                    transferred_amount,
                );
            }
        });
    }
}
