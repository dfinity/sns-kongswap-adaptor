use crate::{
    balances::{Party, ValidatedBalances},
    log_err,
    logged_arithmetics::{logged_saturating_add, logged_saturating_sub},
    state::storage::{ConfigState, StableTransaction},
    validation::ValidatedAsset,
    StableAuditTrail, StableBalances,
};
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::{agent::AbstractAgent, audit::OperationContext};
use sns_treasury_manager::{AuditTrail, Transaction};
use sns_treasury_manager::{Error, Operation, TreasuryManagerOperation};
use std::{cell::RefCell, thread::LocalKey};

pub(crate) mod storage;

const NS_IN_SECOND: u64 = 1_000_000_000;

pub const MAX_LOCK_DURATION_NS: u64 = 45 * 60 * NS_IN_SECOND; // 45 minutes

/// A human-readable name for the owner of the managed funds.
// TODO: Ideally, we would have the name of the owner / SNS.
const TREASURY_OWNER_NAME: &str = "DAO Treasury";

pub(crate) struct KongSwapAdaptor<A: AbstractAgent> {
    time_ns: fn() -> u64,
    pub agent: A,
    pub id: Principal,
    balances: &'static LocalKey<RefCell<StableBalances>>,
    audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
}

impl<A: AbstractAgent> KongSwapAdaptor<A> {
    pub fn new(
        time_ns: fn() -> u64,
        agent: A,
        id: Principal,
        balances: &'static LocalKey<RefCell<StableBalances>>,
        audit_trail: &'static LocalKey<RefCell<StableAuditTrail>>,
    ) -> Self {
        KongSwapAdaptor {
            time_ns,
            agent,
            id,
            balances,
            audit_trail,
        }
    }

    pub fn time_ns(&self) -> u64 {
        (self.time_ns)()
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

            // On each ledger, use the main account and no subaccount for managing the assets.
            let manager_account = Account {
                owner: self.id,
                subaccount: None,
            };

            let timestamp_ns = self.time_ns();

            let owner_name = TREASURY_OWNER_NAME.to_string();

            let validated_balances = ValidatedBalances::new(
                timestamp_ns,
                asset_0,
                asset_1,
                owner_name,
                owner_account_0,
                owner_account_1,
                format!("KongSwapAdaptor({})", self.id),
                manager_account,
                manager_account,
            );

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

    /// Returns a copy of the balances.
    ///
    /// Only safe to call after the canister has been initialized.
    pub fn get_cached_balances(&self) -> ValidatedBalances {
        self.balances.with_borrow(|cell| {
            let ConfigState::Initialized(balances) = cell.get() else {
                ic_cdk::trap("BUG: Balances should be initialized");
            };

            balances.clone()
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

    pub fn charge_fee(&mut self, asset: ValidatedAsset) {
        self.with_balances_mut(|validated_balances| validated_balances.charge_approval_fee(asset));
    }

    pub fn get_asset_for_ledger(&self, canister_id: &String) -> Option<ValidatedAsset> {
        let (asset_0, asset_1) = self.assets();
        if asset_0.ledger_canister_id().to_string() == *canister_id {
            Some(asset_0)
        } else if asset_1.ledger_canister_id().to_string() == *canister_id {
            Some(asset_1)
        } else {
            None
        }
    }

    pub fn move_asset(&mut self, asset: ValidatedAsset, amount: u64, from: Party, to: Party) {
        self.with_balances_mut(|validated_balances| {
            validated_balances.move_asset(asset, from, to, amount)
        });
    }

    pub fn add_manager_balance(&mut self, asset: ValidatedAsset, amount: u64) {
        self.with_balances_mut(|validated_balances| {
            validated_balances.add_manager_balance(asset, amount)
        });
    }

    // Transferred amount includes the ledger fee and the recieved amount
    pub fn find_discrepency(
        &mut self,
        asset: ValidatedAsset,
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

    fn with_audit_trail<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&StableAuditTrail) -> R,
    {
        self.audit_trail.with_borrow(|audit_trail| f(audit_trail))
    }

    fn with_audit_trail_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StableAuditTrail) -> R,
    {
        self.audit_trail
            .with_borrow_mut(|audit_trail| f(audit_trail))
    }

    /// Returns the index of the pushed transaction in the audit trail, or None if the transaction
    /// could not be pushed.
    pub fn push_audit_trail_transaction(&self, transaction: StableTransaction) -> Option<u64> {
        self.with_audit_trail_mut(|audit_trail| {
            let index = audit_trail.len();
            if let Err(err) = audit_trail.push(&transaction) {
                log_err(&format!(
                    "Cannot push transaction to audit trail: {}\ntransaction: {:?}",
                    err, transaction
                ));
                None
            } else {
                Some(index)
            }
        })
    }

    pub fn set_audit_trail_transaction_result(&self, index: u64, transaction: StableTransaction) {
        self.with_audit_trail_mut(|audit_trail| {
            if index < audit_trail.len() {
                audit_trail.set(index, &transaction);
            } else {
                log_err(&format!(
                    "BUG: Invalid index {} for audit trail. Audit trail length: {}",
                    index,
                    audit_trail.len(),
                ));
            }
        });
    }

    pub fn finalize_audit_trail_transaction(&self, context: OperationContext) {
        let index_transaction = self.with_audit_trail(|audit_trail| {
            let num_transactions = audit_trail.len();
            audit_trail
                .iter()
                .rev()
                .enumerate()
                .find_map(|(rev_index, transaction)| {
                    let transaction_operation = transaction.operation;

                    if transaction_operation.operation == context.operation
                        && !transaction_operation.step.is_final
                    {
                        let rev_index: u64 = match rev_index.try_into() {
                            Ok(index) => index,
                            Err(err) => {
                                log_err(&format!(
                                    "BUG: cannot convert usize {} to u64: {}",
                                    rev_index, err
                                ));
                                return None;
                            }
                        };
                        let index = logged_saturating_sub(
                            num_transactions,
                            logged_saturating_add(rev_index, 1),
                        );

                        Some((index, transaction.clone()))
                    } else {
                        None
                    }
                })
        });

        let Some((index, mut transaction)) = index_transaction else {
            log_err(&format!(
                "Audit trail does not have an {} operation that could be finalized. \
                     Operation context: {:?}",
                context.operation.name(),
                context,
            ));
            return;
        };

        transaction.operation.step.is_final = true;

        self.set_audit_trail_transaction_result(index, transaction);
    }

    fn get_remaining_lock_duration_ns(&self) -> Option<u64> {
        let now_ns = self.time_ns();

        fn is_locking_transaction(treasury_manager_operation: &TreasuryManagerOperation) -> bool {
            [Operation::Deposit, Operation::Withdraw]
                .contains(&treasury_manager_operation.operation)
        }

        let AuditTrail { transactions } = self.get_audit_trail();
        let Some(transaction) = transactions
            .iter()
            .rev()
            .find(|transaction| is_locking_transaction(&transaction.treasury_manager_operation))
        else {
            return None;
        };

        if transaction.treasury_manager_operation.step.is_final {
            return None;
        }

        let acquired_timestamp_ns = transaction.timestamp_ns;
        let expiry_timestamp_ns =
            logged_saturating_add(acquired_timestamp_ns, MAX_LOCK_DURATION_NS);

        if now_ns > expiry_timestamp_ns {
            log_err(&format!("Transaction lock expired: {:?}", transaction));
            return None;
        }

        Some(logged_saturating_sub(expiry_timestamp_ns, now_ns))
    }

    /// Checks if the last transaction has been finalized, or if its lock has expired.
    pub fn check_state_lock(&self) -> Result<(), Vec<Error>> {
        if let Some(remaining_lock_duration_ns) = self.get_remaining_lock_duration_ns() {
            return Err(vec![Error::new_temporarily_unavailable(format!(
                "Canister state is locked. Please try again in {} seconds.",
                remaining_lock_duration_ns / NS_IN_SECOND
            ))]);
        }
        Ok(())
    }

    pub fn get_audit_trail(&self) -> AuditTrail {
        let transactions = self
            .audit_trail
            .with_borrow(|audit_trail| audit_trail.iter().map(Transaction::from).collect());

        AuditTrail { transactions }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        state::storage::ConfigState, validation::ValidatedAsset, StableAuditTrail, StableBalances,
        AUDIT_TRAIL_MEMORY_ID, BALANCES_MEMORY_ID,
    };
    use candid::Principal;
    use ic_stable_structures::{
        memory_manager::MemoryManager, Cell as StableCell, DefaultMemoryImpl, Vec as StableVec,
    };
    use icrc_ledger_types::icrc1::account::Account;
    use kongswap_adaptor::{agent::mock_agent::MockAgent, audit::OperationContext};
    use lazy_static::lazy_static;
    use sns_treasury_manager::{
        Asset, Operation, Step, TransactionWitness, TreasuryManagerOperation,
    };
    use std::cell::RefCell;

    thread_local! {
        static TEST_BALANCES: RefCell<StableBalances> = {
            let memory_manager = MemoryManager::init(DefaultMemoryImpl::default());
            let balances_memory = memory_manager.get(BALANCES_MEMORY_ID);
            RefCell::new(StableCell::init(balances_memory, ConfigState::default()).unwrap())
        };

        static TEST_AUDIT_TRAIL: RefCell<StableAuditTrail> = {
            let memory_manager = MemoryManager::init(DefaultMemoryImpl::default());
            let audit_trail_memory = memory_manager.get(AUDIT_TRAIL_MEMORY_ID);
            RefCell::new(StableVec::init(audit_trail_memory).unwrap())
        };
    }

    lazy_static! {
        static ref TREASURY_MANAGER_CANISTER_ID: Principal =
            Principal::from_text("jexlm-gaaaa-aaaar-qalmq-cai").unwrap();
        static ref TEST_ACCOUNT: Account = Account {
            owner: *TREASURY_MANAGER_CANISTER_ID,
            subaccount: None,
        };
        static ref TEST_PRINCIPAL: Principal = Principal::from_text("2vxsx-fae").unwrap();
        static ref TEST_OPERATION: TreasuryManagerOperation = TreasuryManagerOperation {
            operation: Operation::Deposit,
            step: Step {
                index: 0,
                is_final: false,
            },
        };
        static ref TEST_TRANSACTION: StableTransaction = StableTransaction {
            timestamp_ns: 1_000_000_000,
            operation: *TEST_OPERATION,
            canister_id: *TEST_PRINCIPAL,
            result: Ok(TransactionWitness::NonLedger("test".to_string())),
            human_readable: "test".to_string(),
        };
    }

    fn create_test_adaptor() -> KongSwapAdaptor<MockAgent> {
        let mock_agent = MockAgent::new(*TREASURY_MANAGER_CANISTER_ID);
        let canister_id = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap();

        KongSwapAdaptor::new(
            || 1_000_000_000, // Mock timestamp
            mock_agent,
            canister_id,
            &TEST_BALANCES,
            &TEST_AUDIT_TRAIL,
        )
    }

    fn create_test_assets() -> (ValidatedAsset, ValidatedAsset) {
        let asset_0 = ValidatedAsset::try_from(Asset::Token {
            ledger_canister_id: Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai").unwrap(),
            symbol: "ICP".to_string(),
            ledger_fee_decimals: candid::Nat::from(10_000u64),
        })
        .unwrap();

        let asset_1 = ValidatedAsset::try_from(Asset::Token {
            ledger_canister_id: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
            symbol: "SNS".to_string(),
            ledger_fee_decimals: candid::Nat::from(10_000u64),
        })
        .unwrap();

        (asset_0, asset_1)
    }

    #[test]
    fn test_finalize_transaction() {
        let adaptor = create_test_adaptor();
        let (asset_0, asset_1) = create_test_assets();

        // Initialize the adaptor
        adaptor.initialize(asset_0, asset_1, *TEST_ACCOUNT, *TEST_ACCOUNT);

        // Start with an empty audit trail
        {
            let audit_trail_before = adaptor.get_audit_trail();
            assert_eq!(audit_trail_before.transactions.len(), 0);
        }

        // Create a test operation context
        let context = OperationContext::new(Operation::Deposit);

        // Create and push a non-final transaction
        let transaction = TEST_TRANSACTION.clone();

        adaptor.push_audit_trail_transaction(transaction);

        // Verify the transaction is not final initially
        let audit_trail_before = adaptor.get_audit_trail();
        assert_eq!(audit_trail_before.transactions.len(), 1);
        assert!(
            !audit_trail_before.transactions[0]
                .treasury_manager_operation
                .step
                .is_final
        );

        // Finalize the transaction
        adaptor.finalize_audit_trail_transaction(context);

        // Verify the transaction is now finalized
        let audit_trail_after = adaptor.get_audit_trail();
        assert_eq!(audit_trail_after.transactions.len(), 1);
        assert!(
            audit_trail_after.transactions[0]
                .treasury_manager_operation
                .step
                .is_final
        );
    }

    #[test]
    fn test_finalize_transaction_multiple_operations() {
        let adaptor = create_test_adaptor();
        let (asset_0, asset_1) = create_test_assets();

        // Initialize the adaptor
        adaptor.initialize(asset_0, asset_1, *TEST_ACCOUNT, *TEST_ACCOUNT);

        // Start with an empty audit trail
        {
            let audit_trail_before = adaptor.get_audit_trail();
            assert_eq!(audit_trail_before.transactions.len(), 0);
        }

        // Add multiple transactions with different operations
        let deposit_transaction = StableTransaction {
            timestamp_ns: 1_000_000_000,
            operation: TreasuryManagerOperation {
                operation: Operation::Deposit,
                step: Step {
                    index: 0,
                    is_final: false,
                },
            },
            ..TEST_TRANSACTION.clone()
        };

        let withdraw_transaction = StableTransaction {
            timestamp_ns: 2_000_000_000,
            operation: TreasuryManagerOperation {
                operation: Operation::Withdraw,
                step: Step {
                    index: 0,
                    is_final: false,
                },
            },
            ..TEST_TRANSACTION.clone()
        };

        let another_deposit_transaction = StableTransaction {
            timestamp_ns: 3_000_000_000,
            operation: TreasuryManagerOperation {
                operation: Operation::Deposit,
                step: Step {
                    index: 1,
                    is_final: false,
                },
            },
            ..TEST_TRANSACTION.clone()
        };

        adaptor.push_audit_trail_transaction(deposit_transaction);
        adaptor.push_audit_trail_transaction(withdraw_transaction);
        adaptor.push_audit_trail_transaction(another_deposit_transaction);

        {
            let audit_trail = adaptor.get_audit_trail();
            assert_eq!(audit_trail.transactions.len(), 3);
            assert!(
                !audit_trail.transactions[0]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                !audit_trail.transactions[1]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                !audit_trail.transactions[2]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
        }

        adaptor.finalize_audit_trail_transaction(OperationContext::new(Operation::Deposit));

        {
            let audit_trail = adaptor.get_audit_trail();
            assert_eq!(audit_trail.transactions.len(), 3);
            assert!(
                !audit_trail.transactions[0]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                !audit_trail.transactions[1]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                audit_trail.transactions[2]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
        }

        adaptor.finalize_audit_trail_transaction(OperationContext::new(Operation::Withdraw));

        {
            let audit_trail = adaptor.get_audit_trail();
            assert_eq!(audit_trail.transactions.len(), 3);
            assert!(
                !audit_trail.transactions[0]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                audit_trail.transactions[1]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
            assert!(
                audit_trail.transactions[2]
                    .treasury_manager_operation
                    .step
                    .is_final
            );
        }
    }

    #[test]
    fn test_finalize_transaction_no_matching_operation() {
        let adaptor = create_test_adaptor();
        let (asset_0, asset_1) = create_test_assets();

        // Initialize the adaptor
        adaptor.initialize(asset_0, asset_1, *TEST_ACCOUNT, *TEST_ACCOUNT);

        // Start with an empty audit trail
        {
            let audit_trail_before = adaptor.get_audit_trail();
            assert_eq!(audit_trail_before.transactions.len(), 0);
        }

        // Add a deposit transaction
        let deposit_transaction = StableTransaction {
            timestamp_ns: 1_000_000_000,
            operation: TreasuryManagerOperation {
                operation: Operation::Deposit,
                ..*TEST_OPERATION
            },
            ..TEST_TRANSACTION.clone()
        };

        adaptor.push_audit_trail_transaction(deposit_transaction);

        // Try to finalize a withdraw operation (which doesn't exist)
        let withdraw_context = OperationContext::new(Operation::Withdraw);
        adaptor.finalize_audit_trail_transaction(withdraw_context);

        // Verify the deposit transaction remains non-final
        let audit_trail = adaptor.get_audit_trail();
        assert_eq!(audit_trail.transactions.len(), 1);
        assert!(
            !audit_trail.transactions[0]
                .treasury_manager_operation
                .step
                .is_final
        );
    }

    #[test]
    fn test_finalize_transaction_already_final() {
        let adaptor = create_test_adaptor();
        let (asset_0, asset_1) = create_test_assets();

        // Initialize the adaptor
        adaptor.initialize(asset_0, asset_1, *TEST_ACCOUNT, *TEST_ACCOUNT);

        // Start with an empty audit trail
        {
            let audit_trail_before = adaptor.get_audit_trail();
            assert_eq!(audit_trail_before.transactions.len(), 0);
        }

        // Add a transaction that's already final
        let final_transaction = StableTransaction {
            timestamp_ns: 1_000_000_000,
            operation: TreasuryManagerOperation {
                operation: Operation::Deposit,
                step: Step {
                    index: 42,      // Arbitrary index
                    is_final: true, // Already final
                },
            },
            ..TEST_TRANSACTION.clone()
        };

        adaptor.push_audit_trail_transaction(final_transaction);

        // Try to finalize it again
        let context = OperationContext::new(Operation::Deposit);
        adaptor.finalize_audit_trail_transaction(context);

        // Verify it remains final (no change)
        let audit_trail = adaptor.get_audit_trail();
        assert_eq!(audit_trail.transactions.len(), 1);
        assert!(
            audit_trail.transactions[0]
                .treasury_manager_operation
                .step
                .is_final
        );
    }

    #[test]
    fn test_finalize_transaction_empty_audit_trail() {
        let adaptor = create_test_adaptor();
        let (asset_0, asset_1) = create_test_assets();

        // Initialize the adaptor
        adaptor.initialize(asset_0, asset_1, *TEST_ACCOUNT, *TEST_ACCOUNT);

        // Start with an empty audit trail
        {
            let audit_trail_before = adaptor.get_audit_trail();
            assert_eq!(audit_trail_before.transactions.len(), 0);
        }

        // Try to finalize on empty audit trail
        let context = OperationContext::new(Operation::Deposit);
        adaptor.finalize_audit_trail_transaction(context);

        // Verify audit trail remains empty
        let audit_trail = adaptor.get_audit_trail();
        assert_eq!(audit_trail.transactions.len(), 0);
    }
}
