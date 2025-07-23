mod helpers;
mod pocket_ic_agent;

use crate::helpers::{
    compute_treasury_subaccount_bytes, create_kong_adaptor, get_kong_adaptor_wasm,
    install_icp_ledger, install_kong_adaptor, install_sns_ledger, mint_tokens, E8,
    KONGSWAP_BACKEND_CANISTER_ID, NNS_GOVERNANCE_CANISTER_ID, SNS_GOVERNANCE_CANISTER_ID,
    SNS_LEDGER_CANISTER_ID, SNS_ROOT_CANISTER_ID,
};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use pocket_ic::PocketIcBuilder;
use pocket_ic_agent::PocketIcAgent;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{self, AuditTrailRequest};
use std::time::Duration;

#[tokio::test]
async fn broken_dex_test() {
    // Prepare the world.

    let pocket_ic = PocketIcBuilder::new()
        .with_nns_subnet()
        .with_sns_subnet()
        .with_fiduciary_subnet()
        .build_async()
        .await;

    let mut agent = PocketIcAgent::new(pocket_ic);

    let topology = agent.pic().topology().await;
    let fiduciary_subnet_id = topology.get_fiduciary().unwrap();

    // Install the wrong Wasm (e.g., from SNS Ledger) to simulate a broken DEX.
    install_sns_ledger(&agent.pic(), *KONGSWAP_BACKEND_CANISTER_ID).await;

    let sns_ledger_canister_ic = install_sns_ledger(&agent.pic(), *SNS_LEDGER_CANISTER_ID).await;
    let icp_ledger_canister_id = install_icp_ledger(&agent.pic()).await;

    // Install canister under test.
    let kong_adaptor_canister_id = create_kong_adaptor(&agent.pic(), fiduciary_subnet_id).await;

    mint_tokens(
        agent.with_sender(*SNS_ROOT_CANISTER_ID),
        sns_ledger_canister_ic,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        100 * E8,
    )
    .await;

    mint_tokens(
        agent.with_sender(*NNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        100 * E8,
    )
    .await;

    let treasury_icp_account = sns_treasury_manager::Account {
        owner: *SNS_GOVERNANCE_CANISTER_ID,
        subaccount: None, // No subaccount for the SNS treasury.
    };

    let treasury_sns_account = sns_treasury_manager::Account {
        owner: *SNS_GOVERNANCE_CANISTER_ID,
        subaccount: Some(compute_treasury_subaccount_bytes(
            *SNS_GOVERNANCE_CANISTER_ID,
        )),
    };

    let original_wasm = get_kong_adaptor_wasm();

    install_kong_adaptor(
        &agent.pic(),
        original_wasm.clone(),
        kong_adaptor_canister_id,
        treasury_icp_account,
        treasury_sns_account,
        100 * E8,
        100 * E8,
    )
    .await;

    // We need between 50 and 100 ticks to get the initial deposit and the first batch of periodic
    // tasks to be processed.
    for _ in 0..100 {
        agent.pic().advance_time(Duration::from_secs(1)).await;
        agent.pic().tick().await;
    }

    let audit_trail = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
        .unwrap();

    use kongswap_adaptor::audit::serialize_audit_trail;
    panic!(
        "audit_trail = {}",
        serialize_audit_trail(&audit_trail, true).unwrap()
    );
}
