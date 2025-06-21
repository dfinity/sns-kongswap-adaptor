use std::{cell::RefCell, time::Duration};

use candid::Principal;
use ic_canister_log::{declare_log_buffer, log};
use ic_cdk::{init, post_upgrade, query, update};
use sns_treasury_manager::{
    Allowance, AuditTrail, AuditTrailRequest, Balances, BalancesRequest, DepositRequest,
    TransactionError, TreasuryManager, TreasuryManagerArg, TreasuryManagerResult, WithdrawRequest,
};

mod agent;
mod balances;
mod deposit;
mod emit_transaction;
mod kong_api;
mod kong_types;
mod state;
mod validation;
mod withdraw;

use state::KongSwapAdaptor;

use lazy_static::lazy_static;

use crate::validation::{ValidatedDepositRequest, ValidatedTreasuryManagerInit};

const RUN_PERIODIC_TASKS_INTERVAL: Duration = Duration::from_secs(60 * 60); // one hour

// Canister ID from the mainnet.
// See https://dashboard.internetcomputer.org/canister/2ipq2-uqaaa-aaaar-qailq-cai
lazy_static! {
    static ref KONG_BACKEND_CANISTER_ID: Principal =
        Principal::from_text("2ipq2-uqaaa-aaaar-qailq-cai").unwrap();
    static ref ICP_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
}

thread_local! {
    static STATE: RefCell<Option<KongSwapAdaptor>> = RefCell::new(None);
}

fn check_access() {
    let caller = ic_cdk::api::caller();
    if caller != ic_cdk::id() && !ic_cdk::api::is_controller(&caller) {
        ic_cdk::trap("Only a controller can call this method.");
    }
}

declare_log_buffer!(name = LOG, capacity = 100);

fn log_err(msg: &str) {
    log(&format!("Error: {}", msg));
}

fn log(msg: &str) {
    let msg = format!("[KongSwapAdaptor] {}", msg);
    ic_cdk::print(&msg);
    log!(LOG, "{}", msg);
}

impl TreasuryManager for KongSwapAdaptor {
    fn balances(&self, _request: BalancesRequest) -> TreasuryManagerResult {
        Ok(Balances::from(self.get_cached_balances()))
    }

    async fn withdraw(&mut self, _request: WithdrawRequest) -> TreasuryManagerResult {
        self.withdraw_impl().await.map(Balances::from)
    }

    async fn deposit(&mut self, request: DepositRequest) -> TreasuryManagerResult {
        let ValidatedDepositRequest {
            allowance_0,
            allowance_1,
        } = request.try_into().map_err(TransactionError::Precondition)?;

        self.deposit_impl(allowance_0, allowance_1)
            .await
            .map(Balances::from)
    }

    fn audit_trail(&self, _request: AuditTrailRequest) -> AuditTrail {
        self.audit_trail.clone()
    }
}

#[update]
async fn deposit(request: DepositRequest) -> TreasuryManagerResult {
    check_access();

    log("deposit.");

    // TODO: adopt the pattern from NodeRewards canister.
    //
    // Use dependency injections via the pattern, e.g., get_registry_client
    let mut kong_adaptor = STATE.with_borrow_mut(|state| {
        let Some(state) = state.take() else {
            ic_cdk::trap("KongSwapAdaptor is not available.");
        };
        state
    });

    let result = kong_adaptor.deposit(request).await?;

    STATE.with_borrow_mut(|state| {
        *state = Some(kong_adaptor);
    });

    Ok(result)
}

#[update]
async fn withdraw(request: WithdrawRequest) -> TreasuryManagerResult {
    check_access();

    log("withdraw.");

    let mut kong_adaptor = STATE.with_borrow_mut(|state| {
        let Some(state) = state.take() else {
            ic_cdk::trap("KongSwapAdaptor is not available.");
        };
        state
    });

    let result = kong_adaptor.withdraw(request).await;

    let result = result?;

    STATE.with_borrow_mut(|state| {
        *state = Some(kong_adaptor);
    });

    Ok(result)
}

#[query]
fn balances(request: BalancesRequest) -> TreasuryManagerResult {
    let balances = STATE.with_borrow(|state| {
        let Some(state) = state.as_ref() else {
            ic_cdk::trap("KongSwapAdaptor is not available.");
        };
        state.balances(request)
    });

    balances
}

#[query]
fn audit_trail(request: AuditTrailRequest) -> AuditTrail {
    STATE.with_borrow(|state| {
        let Some(state) = state.as_ref() else {
            ic_cdk::trap("KongSwapAdaptor is not available.");
        };
        state.audit_trail(request)
    })
}

async fn run_periodic_tasks() {
    log("run_periodic_tasks.");

    let mut kong_adaptor = STATE.with_borrow_mut(|state| {
        let Some(kong_adaptor) = state.take() else {
            ic_cdk::trap("KongSwapAdaptor is not available.");
        };
        kong_adaptor
    });

    let result = kong_adaptor.refresh_balances().await;

    if let Err(err) = result {
        log_err(&format!(
            "KongSwapAdaptor refresh_balances failed: {:?}",
            err
        ));
    }

    STATE.with_borrow_mut(|state| {
        *state = Some(kong_adaptor);
    });
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
        Err(err) => {
            log_err(&format!(
                "Call failed during async initializition: {:?}",
                err
            ));
            return;
        }
    };

    if let Err(err) = result.0 {
        log_err(&format!("Async initialization failed: {:?}", err));
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

    let kong_adaptor = KongSwapAdaptor::new(
        *KONG_BACKEND_CANISTER_ID,
        allowance_0.asset,
        allowance_1.asset,
    );

    STATE.with_borrow_mut(|state| {
        *state = Some(kong_adaptor);
    });

    init_periodic_tasks();

    let allowance_0 = Allowance::from(allowance_0);
    let allowance_1 = Allowance::from(allowance_1);

    ic_cdk_timers::set_timer(Duration::ZERO, || {
        ic_cdk::spawn(init_async(allowance_0, allowance_1))
    });
}

#[post_upgrade]
fn canister_post_upgrade() {
    log("post_upgrade.");

    init_periodic_tasks();
}

fn main() {
    candid::export_service!();
    println!("{}", __export_service());
}
