mod common;

use candid::Nat;
use maplit::btreemap;
use pocket_ic::PocketIcBuilder;
use pretty_assertions::assert_eq;

use crate::common::{
    pocket_ic_agent::PocketIcAgent,
    utils::{
        create_kong_adaptor, get_balance, get_kong_adaptor_wasm, install_icp_ledger,
        install_kong_swap, install_sns_ledger, setup_kongswap_adaptor, withdraw, E8, FEE_ICP,
        FEE_SNS, WITHDRAW_ACCOUNT_0, WITHDRAW_ACCOUNT_1,
    },
};

#[tokio::test]
async fn withdraw_test() {
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

    let original_wasm = get_kong_adaptor_wasm();

    let initial_deposit_sns = 100 * E8;
    let initial_deposit_icp = 100 * E8;

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

    {
        let withdraw_accounts = btreemap! {
            sns_ledger_canister_id => *WITHDRAW_ACCOUNT_0,
            icp_ledger_canister_id => *WITHDRAW_ACCOUNT_1,
        };

        let (balances_0, balances_1) = withdraw(
            &mut agent,
            kong_adaptor_canister_id,
            Some(withdraw_accounts),
        )
        .await;

        assert_eq!(
            balances_0.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(5 * FEE_SNS)
        );
        assert_eq!(
            balances_1.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(5 * FEE_ICP)
        );

        assert_eq!(
            balances_0
                .external_custodian
                .as_ref()
                .unwrap()
                .amount_decimals,
            Nat::from(0_u64),
            "There should be no SNS left in the DEX"
        );
        assert_eq!(
            balances_1
                .external_custodian
                .as_ref()
                .unwrap()
                .amount_decimals,
            Nat::from(0_u64),
            "There should be no ICP left in the DEX"
        );

        let withdraw_account_0_balance =
            get_balance(&mut agent, sns_ledger_canister_id, *WITHDRAW_ACCOUNT_0).await;
        let withdraw_account_1_balance =
            get_balance(&mut agent, icp_ledger_canister_id, *WITHDRAW_ACCOUNT_1).await;

        // Fees:
        // I. deposit
        //      1. transfer fee from the SNS to the adaptor
        //      2. approval fee from the adaptor
        //      3. transfer fee from the adaptor to the DEX
        // II. withdraw
        //      4. transfer fee from the DEX to the adaptor
        //      5. transfer fee from the adaptor to the withdraw account

        // Approval fee from the SNS is not counted, as when setting up the approval
        // we have given approval for the initial_deposit_sns + FEE_SNS and
        // initial_deposit_icp + FEE_ICP
        assert_eq!(
            withdraw_account_0_balance,
            Nat::from(initial_deposit_sns - 5 * FEE_SNS)
        );
        assert_eq!(
            withdraw_account_1_balance,
            Nat::from(initial_deposit_icp - 5 * FEE_ICP)
        );
    }
}
