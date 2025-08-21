mod common;

use candid::Nat;
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::PocketIcBuilder;
use pretty_assertions::assert_eq;
use std::time::Duration;

use crate::common::{
    pocket_ic_agent::PocketIcAgent,
    utils::{
        approve_tokens, create_kong_adaptor, get_balance, get_kong_adaptor_wasm,
        install_icp_ledger, install_kong_adaptor, install_kong_swap, install_sns_ledger,
        mint_tokens, E8, FEE_ICP, FEE_SNS, NNS_GOVERNANCE_CANISTER_ID, SNS_GOVERNANCE_CANISTER_ID,
        TREASURY_ICP_ACCOUNT, TREASURY_SNS_ACCOUNT,
    },
};

#[tokio::test]
async fn allowance_test() {
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
    let initial_deposit_sns = 100 * E8;
    let initial_deposit_icp = 50 * E8;

    mint_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: TREASURY_SNS_ACCOUNT.owner.clone(),
            subaccount: TREASURY_SNS_ACCOUNT.subaccount.clone(),
        },
        initial_deposit_sns,
    )
    .await;

    approve_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        initial_deposit_sns,
        FEE_SNS,
        TREASURY_SNS_ACCOUNT.subaccount.clone(),
    )
    .await;

    mint_tokens(
        agent.with_sender(*NNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner: TREASURY_ICP_ACCOUNT.owner.clone(),
            subaccount: TREASURY_ICP_ACCOUNT.subaccount.clone(),
        },
        initial_deposit_icp,
    )
    .await;

    approve_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        initial_deposit_icp,
        FEE_ICP,
        TREASURY_ICP_ACCOUNT.subaccount.clone(),
    )
    .await;

    let original_wasm = get_kong_adaptor_wasm();

    install_kong_adaptor(
        &agent.pic(),
        original_wasm.clone(),
        kong_adaptor_canister_id,
        *TREASURY_ICP_ACCOUNT,
        *TREASURY_SNS_ACCOUNT,
        initial_deposit_icp,
        initial_deposit_sns,
    )
    .await;

    for _ in 0..10 {
        agent.pic().tick().await;
    }

    agent.pic().advance_time(Duration::from_secs(4000)).await;

    // We need between 50 and 100 ticks to get the initial deposit and the first batch of periodic
    // tasks to be processed.
    for _ in 0..90 {
        agent.pic().tick().await;
    }
    // Here, the initialisation must have failed due to expired allowances.
    // Hence, we should have our assets back minus 2 fees (one approval fee + one returning fee)
    let governance_sns_balance =
        get_balance(&mut agent, sns_ledger_canister_id, *TREASURY_SNS_ACCOUNT).await;
    let governance_icp_balance =
        get_balance(&mut agent, icp_ledger_canister_id, *TREASURY_ICP_ACCOUNT).await;

    assert_eq!(
        governance_sns_balance,
        Nat::from(initial_deposit_sns - 4 * FEE_SNS)
    );
    assert_eq!(
        governance_icp_balance,
        Nat::from(initial_deposit_icp - 4 * FEE_ICP)
    );
}
