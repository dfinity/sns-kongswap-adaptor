mod common;

use kongswap_adaptor::agent::AbstractAgent;
use pocket_ic::PocketIcBuilder;
use pretty_assertions::assert_eq;
use sns_treasury_manager::{self, AuditTrailRequest, BalancesRequest, TreasuryManagerUpgrade};

use crate::common::{
    pocket_ic_agent::PocketIcAgent,
    utils::{
        create_kong_adaptor, get_kong_adaptor_wasm, install_icp_ledger, install_kong_swap,
        install_sns_ledger, setup_kongswap_adaptor, E8, FEE_ICP, FEE_SNS,
        SNS_GOVERNANCE_CANISTER_ID,
    },
};

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

    install_kong_swap(&agent.pic()).await;
    let sns_ledger_canister_id = install_sns_ledger(&agent.pic()).await;
    let icp_ledger_canister_id = install_icp_ledger(&agent.pic()).await;

    // Install canister under test.
    let kong_adaptor_canister_id = create_kong_adaptor(&agent.pic(), fiduciary_subnet_id).await;

    let initial_deposit_sns = 100 * E8 + FEE_SNS;
    let initial_deposit_icp = 100 * E8 + FEE_ICP;

    let original_wasm = get_kong_adaptor_wasm();

    setup_kongswap_adaptor(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        &original_wasm,
        initial_deposit_sns,
        initial_deposit_icp,
    )
    .await;

    let balances_before_upgrade = agent
        .call(kong_adaptor_canister_id, BalancesRequest {})
        .await
        .unwrap()
        .unwrap();

    let module_hash_before_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_GOVERNANCE_CANISTER_ID))
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
            Some(*SNS_GOVERNANCE_CANISTER_ID),
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
        .canister_status(kong_adaptor_canister_id, Some(*SNS_GOVERNANCE_CANISTER_ID))
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
            Some(*SNS_GOVERNANCE_CANISTER_ID),
        )
        .await
        .unwrap();

    let module_hash_after_second_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_GOVERNANCE_CANISTER_ID))
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

    use kongswap_adaptor::audit::serialize_audit_trail;
    println!(
        "audit_trail = {}",
        serialize_audit_trail(&audit_trail_after_second_upgrade, true).unwrap()
    );
}
