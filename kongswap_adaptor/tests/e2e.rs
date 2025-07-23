mod helpers;
mod pocket_ic_agent;

use crate::helpers::{
    compute_treasury_subaccount_bytes, create_kong_adaptor, get_kong_adaptor_wasm,
    install_icp_ledger, install_kong_adaptor, install_kong_swap, install_sns_ledger, mint_tokens,
    E8, NNS_GOVERNANCE_CANISTER_ID, SNS_GOVERNANCE_CANISTER_ID, SNS_LEDGER_CANISTER_ID,
    SNS_ROOT_CANISTER_ID,
};
use icrc_ledger_types::icrc1::account::Account;
use kongswap_adaptor::agent::AbstractAgent;
use pocket_ic::PocketIcBuilder;
use pocket_ic_agent::PocketIcAgent;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{self, AuditTrailRequest, BalancesRequest, TreasuryManagerUpgrade};
use std::time::Duration;

#[tokio::test]
async fn e2e_test() {
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

    let _kong_backend_canister_id = install_kong_swap(&agent.pic()).await;
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

    let balances_before_upgrade = agent
        .call(kong_adaptor_canister_id, BalancesRequest {})
        .await
        .unwrap()
        .unwrap();

    let module_hash_before_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_ROOT_CANISTER_ID))
        .await
        .unwrap()
        .module_hash
        .unwrap();

    let audit_trail_before_upgrade = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
        .unwrap();

    let modified_wasm = original_wasm.clone().modified();

    // 1st upgrade. Tests the the post-upgrade hook.
    agent
        .pic()
        .upgrade_canister(
            kong_adaptor_canister_id,
            modified_wasm.bytes(),
            candid::encode_one(&sns_treasury_manager::TreasuryManagerArg::Upgrade(
                TreasuryManagerUpgrade {},
            ))
            .unwrap(),
            Some(*SNS_ROOT_CANISTER_ID),
        )
        .await
        .unwrap();

    // Should be called before balances, since the latter affects the audit trail.
    let audit_trail_after_upgrade = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
        .unwrap();

    let module_hash_after_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_ROOT_CANISTER_ID))
        .await
        .unwrap()
        .module_hash
        .unwrap();

    let balances_after_upgrade = agent
        .call(kong_adaptor_canister_id, BalancesRequest {})
        .await
        .unwrap()
        .unwrap();

    assert_ne!(module_hash_after_upgrade, module_hash_before_upgrade);
    assert_eq!(balances_after_upgrade, balances_before_upgrade);
    assert_eq!(audit_trail_after_upgrade, audit_trail_before_upgrade);

    // 2nd upgrade. Tests the pre-upgrade hook.
    agent
        .pic()
        .upgrade_canister(
            kong_adaptor_canister_id,
            original_wasm.bytes(),
            candid::encode_one(&sns_treasury_manager::TreasuryManagerArg::Upgrade(
                TreasuryManagerUpgrade {},
            ))
            .unwrap(),
            Some(*SNS_ROOT_CANISTER_ID),
        )
        .await
        .unwrap();

    let module_hash_after_second_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_ROOT_CANISTER_ID))
        .await
        .unwrap()
        .module_hash
        .unwrap();

    let balances_after_second_upgrade = agent
        .call(kong_adaptor_canister_id, BalancesRequest {})
        .await
        .unwrap()
        .unwrap();

    let audit_trail_after_second_upgrade = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
        .unwrap();

    assert_eq!(module_hash_after_second_upgrade, module_hash_before_upgrade);
    assert_eq!(balances_after_second_upgrade, balances_before_upgrade);
    assert_eq!(audit_trail_after_second_upgrade, audit_trail_before_upgrade);

    // use kongswap_adaptor::audit::serialize_audit_trail;
    // panic!(
    //     "audit_trail = {}",
    //     serialize_audit_trail(&audit_trail_after_second_upgrade, true).unwrap()
    // );
}
