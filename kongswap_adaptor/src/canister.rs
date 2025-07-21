use crate::state::storage::{ConfigState, StableTransaction};
use crate::validation::{
    ValidatedDepositRequest, ValidatedTreasuryManagerInit, ValidatedWithdrawRequest,
};
use candid::Principal;
use ic_canister_log::{declare_log_buffer, log};
use ic_cdk::{init, post_upgrade, pre_upgrade, query, update};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{Cell as StableCell, DefaultMemoryImpl, Vec as StableVec};
use kongswap_adaptor::agent::ic_cdk_agent::CdkAgent;
use kongswap_adaptor::agent::AbstractAgent;
use kongswap_adaptor::audit::OperationContext;
use lazy_static::lazy_static;
use sns_treasury_manager::{
    Allowance, AuditTrail, AuditTrailRequest, Balances, BalancesRequest, DepositRequest, Error,
    Operation, TreasuryManager, TreasuryManagerArg, TreasuryManagerResult, WithdrawRequest,
};
use state::KongSwapAdaptor;
use std::sync::Arc;
use std::{cell::RefCell, time::Duration};

mod balances;
mod deposit;
mod emit_transaction;
mod kong_api;
mod kong_types;
mod ledger_api;
mod rewards;
mod state;
mod tx_error_codes;
mod validation;
mod withdraw;

const RUN_PERIODIC_TASKS_INTERVAL: Duration = Duration::from_secs(60 * 60); // one hour

pub(crate) type Memory = VirtualMemory<DefaultMemoryImpl>;
pub(crate) type StableAuditTrail = StableVec<StableTransaction, Memory>;
pub(crate) type StableBalances = StableCell<ConfigState, Memory>;

// Canister ID from the mainnet.
// See https://dashboard.internetcomputer.org/canister/2ipq2-uqaaa-aaaar-qailq-cai
lazy_static! {
    static ref KONG_BACKEND_CANISTER_ID: Principal =
        Principal::from_text("2ipq2-uqaaa-aaaar-qailq-cai").unwrap();
    static ref ICP_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
}

const BALANCES_MEMORY_ID: MemoryId = MemoryId::new(0);
const AUDIT_TRAIL_MEMORY_ID: MemoryId = MemoryId::new(1);

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    static BALANCES: RefCell<StableBalances> =
        MEMORY_MANAGER.with(|memory_manager|
            RefCell::new(
                StableCell::init(
                    memory_manager.borrow().get(BALANCES_MEMORY_ID),
                    ConfigState::default()
                )
                .expect("BALANCES init should not cause errors")
            )
        );

    static AUDIT_TRAIL: RefCell<StableAuditTrail> =
        MEMORY_MANAGER.with(|memory_manager|
            RefCell::new(
                StableVec::init(
                    memory_manager.borrow().get(AUDIT_TRAIL_MEMORY_ID)
                )
                .expect("AUDIT_TRAIL init should not cause errors")
            )
        );

}

fn time_ns() -> u64 {
    ic_cdk::api::time()
}

fn canister_state() -> KongSwapAdaptor<CdkAgent> {
    KongSwapAdaptor::new(
        Box::new(time_ns),
        Arc::new(CdkAgent::new()),
        ic_cdk::id(),
        &BALANCES,
        &AUDIT_TRAIL,
    )
}

fn check_access() {
    let caller = ic_cdk::api::caller();

    if caller == ic_cdk::id() {
        return;
    }

    if ic_cdk::api::is_controller(&caller) {
        return;
    }

    ic_cdk::trap("Only a controller can call this method.");
}

declare_log_buffer!(name = LOG, capacity = 100);

fn log_err(msg: &str) {
    log(&format!("Error: {}", msg));
}

fn log(msg: &str) {
    let msg = format!("[KongSwapAdaptor] {}", msg);

    if cfg!(target_arch = "wasm32") {
        ic_cdk::print(&msg);
    } else {
        println!("{}", msg);
    }

    log!(LOG, "{}", msg);
}

impl<A: AbstractAgent> TreasuryManager for KongSwapAdaptor<A> {
    async fn withdraw(&mut self, request: WithdrawRequest) -> TreasuryManagerResult {
        self.check_state_lock()?;

        let (ledger_0, ledger_1) = self.ledgers();

        let (default_owner_0, default_owner_1) = self.owner_accounts();

        let ValidatedWithdrawRequest {
            withdraw_account_0,
            withdraw_account_1,
        } = (
            ledger_0,
            ledger_1,
            default_owner_0,
            default_owner_1,
            request,
        )
            .try_into()
            .map_err(|err: String| vec![Error::new_precondition(err)])?;

        let mut context = OperationContext::new(Operation::Withdraw);

        let returned_amounts = self
            .withdraw_impl(&mut context, withdraw_account_0, withdraw_account_1)
            .await
            .map(Balances::from)?;

        self.finalize_audit_trail_transaction(context);

        Ok(returned_amounts)
    }

    async fn deposit(&mut self, request: DepositRequest) -> TreasuryManagerResult {
        self.check_state_lock()?;

        let ValidatedDepositRequest {
            allowance_0,
            allowance_1,
        } = request
            .try_into()
            .map_err(|err: String| vec![Error::new_precondition(err)])?;

        self.validate_deposit_args(allowance_0, allowance_1)
            .map_err(|err| vec![err])?;

        let mut context = OperationContext::new(Operation::Deposit);

        let deposited_amounts = self
            .deposit_impl(&mut context, allowance_0, allowance_1)
            .await
            .map(Balances::from)?;

        self.finalize_audit_trail_transaction(context);

        Ok(deposited_amounts)
    }

    fn audit_trail(&self, _request: AuditTrailRequest) -> AuditTrail {
        self.get_audit_trail()
    }

    fn balances(&self, _request: BalancesRequest) -> TreasuryManagerResult {
        Ok(Balances::from(self.get_cached_balances()))
    }

    async fn refresh_balances(&mut self) {
        if let Err(err) = self.check_state_lock() {
            log_err(&format!("Cannot refresh balances: {:?}", err));
            return;
        }

        let mut context = OperationContext::new(Operation::Balances);

        let result = self.refresh_balances_impl(&mut context).await;

        if let Err(err) = result {
            log_err(&format!("refresh_balances failed: {:?}", err));
        }

        self.finalize_audit_trail_transaction(context);
    }

    async fn issue_rewards(&mut self) {
        if let Err(err) = self.check_state_lock() {
            log_err(&format!("Cannot issue rewards: {:?}", err));
            return;
        }

        let mut context = OperationContext::new(Operation::IssueReward);

        let result = self.issue_rewards_impl(&mut context).await;

        if let Err(err) = result {
            log_err(&format!("issue_rewards failed: {:?}", err));
        }

        self.finalize_audit_trail_transaction(context);
    }
}

#[update]
async fn deposit(request: DepositRequest) -> TreasuryManagerResult {
    check_access();

    log("deposit.");

    let result = canister_state().deposit(request).await?;

    Ok(result)
}

#[update]
async fn withdraw(request: WithdrawRequest) -> TreasuryManagerResult {
    check_access();

    log("withdraw.");

    let result = canister_state().withdraw(request).await?;

    Ok(result)
}

#[query]
fn balances(request: BalancesRequest) -> TreasuryManagerResult {
    canister_state().balances(request)
}

#[query]
fn audit_trail(request: AuditTrailRequest) -> AuditTrail {
    canister_state().audit_trail(request)
}

async fn run_periodic_tasks() {
    log("run_periodic_tasks.");

    let mut kong_adaptor = canister_state();

    kong_adaptor.refresh_balances().await;

    kong_adaptor.issue_rewards().await;
}

fn init_periodic_tasks() {
    let _new_timer_id = ic_cdk_timers::set_timer_interval(RUN_PERIODIC_TASKS_INTERVAL, || {
        ic_cdk::spawn(run_periodic_tasks())
    });
}

async fn init_async(allowance_0: Allowance, allowance_1: Allowance) {
    log("init_async.");

    let request = DepositRequest {
        allowances: vec![allowance_0, allowance_1],
    };

    let result: Result<(TreasuryManagerResult,), _> =
        ic_cdk::call(ic_cdk::id(), "deposit", (request,)).await;

    let result = match result {
        Ok(result) => result,
        Err((err_code, err_message)) => {
            log_err(&format!(
                "Self-call failed in async initializition. Error code {}: {:?}",
                err_code as i32, err_message,
            ));
            return;
        }
    };

    if let Err(err) = result.0 {
        log_err(&format!("Initial deposit failed: {:?}", err));
        return;
    }

    // Ensure the balances are available after initialization.
    run_periodic_tasks().await;

    log("init_async completed successfully.");
}

#[init]
async fn canister_init(arg: TreasuryManagerArg) {
    log("init.");

    let TreasuryManagerArg::Init(init) = arg else {
        ic_cdk::trap("Expected TreasuryManagerArg::Init on canister install.");
    };

    let ValidatedTreasuryManagerInit {
        allowance_0,
        allowance_1,
    } = init
        .try_into()
        .expect("Failed to validate TreasuryManagerInit.");

    canister_state().initialize(
        allowance_0.asset,
        allowance_1.asset,
        allowance_0.owner_account,
        allowance_1.owner_account,
    );

    init_periodic_tasks();

    let allowance_0 = Allowance::from(allowance_0);
    let allowance_1 = Allowance::from(allowance_1);

    ic_cdk_timers::set_timer(Duration::ZERO, || {
        ic_cdk::spawn(init_async(allowance_0, allowance_1))
    });
}

#[pre_upgrade]
fn canister_pre_upgrade() {
    log("pre_upgrade.");
}

#[post_upgrade]
fn canister_post_upgrade(arg: TreasuryManagerArg) {
    log("post_upgrade.");

    let TreasuryManagerArg::Upgrade(_upgrade) = arg else {
        ic_cdk::trap("Expected TreasuryManagerArg::Upgrade on canister upgrade.");
    };

    init_periodic_tasks();
}

/// Used in order to commit the canister state, which requires an inter-canister call.
/// Otherwise, a trap could discard the state mutations, complicating recovery.
/// See: https://internetcomputer.org/docs/building-apps/security/inter-canister-calls#journaling
#[update(hidden = true)]
fn commit_state() {
    check_access();
}

fn candid_service() -> String {
    candid::export_service!();
    __export_service()
}

fn main() {
    candid::export_service!();
    println!("{}", candid_service());
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../kongswap-adaptor.did");
        let declared_interface = CandidSource::Text(declared_interface);

        // candid::export_service!();
        let implemented_interface_str = candid_service();
        let implemented_interface = CandidSource::Text(&implemented_interface_str);

        let result = service_equal(declared_interface, implemented_interface);
        assert!(result.is_ok(), "{:?}\n\n", result.unwrap_err());
    }
}
