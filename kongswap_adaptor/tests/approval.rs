mod common;

use candid::{Decode, Encode, Nat};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::PocketIcBuilder;
use pretty_assertions::assert_eq;
use std::time::Duration;

use crate::common::{
    pocket_ic_agent::PocketIcAgent,
    utils::{
        approve_tokens, create_kong_adaptor, install_sns_ledger, mint_tokens, E8, FEE_SNS,
        SNS_GOVERNANCE_CANISTER_ID, TREASURY_SNS_ACCOUNT,
    },
};

#[tokio::test]
async fn allowance_overwrite_test() {
    // Prepare the world.
    let pocket_ic = PocketIcBuilder::new()
        .with_nns_subnet()
        .with_sns_subnet()
        .with_fiduciary_subnet()
        .build_async()
        .await;

    let mut agent = PocketIcAgent::new(pocket_ic);
    let sns_amount = 100 * E8;
    let sns_approval = 10 * E8;

    let topology = agent.pic().topology().await;
    let fiduciary_subnet_id = topology.get_fiduciary().unwrap();

    let kong_adaptor_canister_id = create_kong_adaptor(&agent.pic(), fiduciary_subnet_id).await;
    let sns_ledger_canister_id = install_sns_ledger(&agent.pic()).await;

    for _ in 0..10 {
        agent.pic().tick().await;
        agent.pic().advance_time(Duration::from_secs(10)).await;
    }

    mint_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: TREASURY_SNS_ACCOUNT.owner.clone(),
            subaccount: TREASURY_SNS_ACCOUNT.subaccount.clone(),
        },
        sns_amount,
    )
    .await;

    for _ in 0..10 {
        agent.pic().tick().await;
        agent.pic().advance_time(Duration::from_secs(10)).await;
    }

    approve_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        sns_approval,
        FEE_SNS,
        TREASURY_SNS_ACCOUNT.subaccount.clone(),
    )
    .await;

    for _ in 0..10 {
        agent.pic().tick().await;
        agent.pic().advance_time(Duration::from_secs(10)).await;
    }

    approve_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        sns_approval,
        FEE_SNS,
        TREASURY_SNS_ACCOUNT.subaccount.clone(),
    )
    .await;

    for _ in 0..10 {
        agent.pic().tick().await;
        agent.pic().advance_time(Duration::from_secs(10)).await;
    }

    let request = icrc_ledger_types::icrc2::allowance::AllowanceArgs {
        account: Account {
            owner: TREASURY_SNS_ACCOUNT.owner.clone(),
            subaccount: TREASURY_SNS_ACCOUNT.subaccount.clone(),
        },
        spender: Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
    };

    let allowance = agent
        .pic()
        .query_call(
            sns_ledger_canister_id.into(),
            *SNS_GOVERNANCE_CANISTER_ID,
            "icrc2_allowance",
            Encode!(&request).unwrap(),
        )
        .await
        .unwrap();

    let allowance = Decode!(&allowance, icrc_ledger_types::icrc2::allowance::Allowance).unwrap();

    assert_eq!(allowance.allowance, Nat::from(sns_approval - FEE_SNS));
}
