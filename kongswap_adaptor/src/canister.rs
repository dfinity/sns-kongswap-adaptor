use crate::{
    agent::{ic_cdk_agent::CdkAgent, AbstractAgent},
    validation::{ValidatedDepositRequest, ValidatedTreasuryManagerInit, ValidatedWithdrawRequest},
};
use candid::Principal;
use ic_canister_log::{declare_log_buffer, log};
use ic_cdk::{init, post_upgrade, query, update};
use lazy_static::lazy_static;
use sns_treasury_manager::{
    Allowance, AuditTrail, AuditTrailRequest, Balances, BalancesRequest, DepositRequest,
    TransactionError, TreasuryManager, TreasuryManagerArg, TreasuryManagerResult, WithdrawRequest,
};
use state::KongSwapAdaptor;
use std::{cell::RefCell, time::Duration};

mod agent;
mod balances;
mod deposit;
mod emit_transaction;
mod kong_api;
mod kong_types;
mod ledger_api;
mod rewards;
mod state;
mod validation;
mod withdraw;

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
    static STATE: RefCell<Option<KongSwapAdaptor<CdkAgent>>> = RefCell::new(None);
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
    ic_cdk::print(&msg);
    log!(LOG, "{}", msg);
}

impl<A: AbstractAgent> TreasuryManager for KongSwapAdaptor<A> {
    async fn withdraw(&mut self, request: WithdrawRequest) -> TreasuryManagerResult {
        let ledger_0 = self.balances.asset_0.ledger_canister_id();
        let ledger_1 = self.balances.asset_1.ledger_canister_id();
        let default_withdraw_account_0 = self.balances.owner_account_0;
        let default_withdraw_account_1 = self.balances.owner_account_1;

        let ValidatedWithdrawRequest {
            withdraw_account_0,
            withdraw_account_1,
        } = (
            ledger_0,
            ledger_1,
            default_withdraw_account_0,
            default_withdraw_account_1,
            request,
        )
            .try_into()
            .map_err(|err| vec![TransactionError::Precondition(err)])?;

        let returned_amounts = self
            .withdraw_impl(withdraw_account_0, withdraw_account_1)
            .await
            .map(Balances::from)?;

        Ok(returned_amounts)
    }

    async fn deposit(&mut self, request: DepositRequest) -> TreasuryManagerResult {
        let ValidatedDepositRequest {
            allowance_0,
            allowance_1,
        } = request
            .try_into()
            .map_err(|err| vec![TransactionError::Precondition(err)])?;

        let deposited_amounts = self
            .deposit_impl(allowance_0, allowance_1)
            .await
            .map(Balances::from)?;

        Ok(deposited_amounts)
    }

    fn audit_trail(&self, _request: AuditTrailRequest) -> AuditTrail {
        self.audit_trail.clone()
    }

    fn balances(&self, _request: BalancesRequest) -> TreasuryManagerResult {
        Ok(Balances::from(self.get_cached_balances()))
    }

    async fn refresh_balances(&mut self) {
        let result = self.refresh_balances_impl().await;

        if let Err(err) = result {
            log_err(&format!(
                "KongSwapAdaptor refresh_balances failed: {:?}",
                err
            ));
        }
    }

    async fn issue_rewards(&mut self) {
        let result = self.issue_rewards_impl().await;

        if let Err(err) = result {
            log_err(&format!("KongSwapAdaptor issue_rewards failed: {:?}", err));
        }
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

    kong_adaptor.refresh_balances().await;

    kong_adaptor.issue_rewards().await;

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

    let kong_adaptor = KongSwapAdaptor::new(
        CdkAgent::new(),
        *KONG_BACKEND_CANISTER_ID,
        allowance_0.asset,
        allowance_1.asset,
        allowance_0.owner_account,
        allowance_1.owner_account,
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
fn canister_post_upgrade(arg: TreasuryManagerArg) {
    log("post_upgrade.");

    let TreasuryManagerArg::Upgrade(_upgrade) = arg else {
        ic_cdk::trap("Expected TreasuryManagerArg::Upgrade on canister upgrade.");
    };

    init_periodic_tasks();
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
