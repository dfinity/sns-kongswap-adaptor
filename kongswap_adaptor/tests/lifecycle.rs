mod common;

use candid::Nat;
use pocket_ic::PocketIcBuilder;
use pretty_assertions::assert_eq;

use crate::common::{
    pocket_ic_agent::PocketIcAgent,
    utils::{
        create_kong_adaptor, deposit, get_kong_adaptor_wasm, install_icp_ledger, install_kong_swap,
        install_sns_ledger, setup_kongswap_adaptor, trade, withdraw, E8, FEE_ICP, FEE_SNS,
        TREASURY_ICP_ACCOUNT, TREASURY_SNS_ACCOUNT,
    },
};

#[tokio::test]
async fn lifecycle_test() {
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

    // Phase I: setup the canister
    // When we freshly deploy the pool, the desired amounts of both
    // tokens are moved to the pool. Therefore, we expect
    // SNS:
    //      reserve_sns = 100 * E8 - 2 * FEE_SNS
    // ICP:
    //      reserve_icp = 100 * E8 - 2 * FEE_ICP
    setup_kongswap_adaptor(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        &original_wasm,
        100 * E8,
        100 * E8,
    )
    .await;

    // Phase II: another deposit
    // As the second deposit is proportional to the existing reserves
    // of the pool, it gets thoroughly transferred to the pool
    // SNS:
    //      reserve_sns += 10 - 2 * FEE_SNS
    // ICP:
    //      reserve_icp += 10 - 2 * FEE_ICP
    deposit(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        *TREASURY_SNS_ACCOUNT,
        *TREASURY_ICP_ACCOUNT,
        10 * E8,
    )
    .await;
    // Phase III: first trade: a DEX user buys some SNS tokens.
    // Here, we try to swap 1 * E8 of SNS and receive ICP in return.
    // The amount of ICP withdrawn from the kongswap backend is
    // amount_icp = amount_sns * reserve_icp / (reserve_sns + amount_sns)
    // where amount_sns = 1 * E8 - 2 * FEE_SNS
    // SNS:
    //      reserve_sns += amount_sns
    // ICP:
    //      reserve_icp -= amount_icp
    //      lp_fee_1 = 30 * amount_icp / 10_000
    // When selling out a token, a tiny portion of it goes to the liquidity
    // provider. Here, the trader receives `amount_icp - lp_fee_1`.
    trade(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        1 * E8,
        true,
    )
    .await;
    // Phase IV: another deposit
    // Now, as the pool is no longer in 1:1 price point, and we have more SNS
    // some amount of ICP will be returned to the user (one more fee)
    // The amount of icp to be moved into the pool is calculated as
    // amount_icp = (reserve_icp / reserve_sns) * deposit_amount
    // where depoit_amount = 10 * E8 - 2 * FEE_SNS
    // SNS:
    //      reserve_0 += deposit_amount
    // ICP:
    //      reserve_1 += amount_icp calculated above
    deposit(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        *TREASURY_SNS_ACCOUNT,
        *TREASURY_ICP_ACCOUNT,
        10 * E8,
    )
    .await;
    // Phase V: second trade: a DEX user buys some SNS tokens.
    // Here, we try to swap 1 * E8 of ICP and receive SNS in return.
    // The amount of SNS withdrawn from the kongswap backend is
    // amount_sns = amount_icp * reserve_sns / (reserve_icp + amount_icp)
    // where amount_icp = 1 * E8 - 2 * FEE_ICP
    // SNS:
    //      reserve_sns -= amount_sns
    //      lp_fee_0 = 30 * amount_sns / 10_000
    // When selling out a token, a tiny portion of it goes to the liquidity
    // provider. Here, the trader receives `amount_sns - lp_fee_0`.
    // ICP:
    //      reserve_icp += E8
    //      lp_fee_1 = 30 * amount_icp / 10_000
    trade(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        1 * E8,
        false,
    )
    .await;

    // VI: final phase: withdrawal
    {
        let (balances_0, balances_1) = withdraw(&mut agent, kong_adaptor_canister_id).await;
        assert_eq!(
            balances_0.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(11 * FEE_SNS)
        );
        assert_eq!(
            balances_1.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(12 * FEE_ICP)
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

        // Following the calculations above, we get the following values for the pool
        // reserve_0: 11998963701, lp_fee_0: 302868, reserve_1: 11982907522, lp_fee_1: 297238
        // means that the treasury owner receives:
        // SNS => reserve_0 + lp_fee_0 - 2 * FEE_SNS = 11999246569
        // and
        // ICP => reserve_1 + lp_fee_1 - 2 * FEE_ICP = 11983184760
        fn decode_nat_to_u64(value: Nat) -> Result<u64, String> {
            let u64_digit_components = value.0.to_u64_digits();

            match &u64_digit_components[..] {
                [] => Ok(0),
                [val] => Ok(*val),
                vals => Err(format!(
            "Error parsing a Nat value `{:?}` to u64: expected a unique u64 value, got {:?}.",
            &value,
            vals.len(),
        )),
            }
        }

        fn is_within_tolerance(expected: f64, observed: f64, tolerance: f64) -> bool {
            (expected - observed).abs() <= expected.abs() * tolerance
        }

        let error_tolerance = 0.000001;

        assert!(is_within_tolerance(
            (11999246569_u64 - 6 * FEE_SNS) as f64,
            decode_nat_to_u64(
                balances_0
                    .treasury_owner
                    .as_ref()
                    .unwrap()
                    .amount_decimals
                    .clone()
            )
            .unwrap() as f64,
            error_tolerance
        ));

        // As we now use ICRC2 for initialisations and deposits,
        // for each one, we pay 2 extra fees: one approval + one transfer fee.
        assert!(is_within_tolerance(
            (12001107784_u64 - 6 * FEE_ICP) as f64,
            decode_nat_to_u64(
                balances_1
                    .treasury_owner
                    .as_ref()
                    .unwrap()
                    .amount_decimals
                    .clone()
            )
            .unwrap() as f64,
            error_tolerance
        ));
    }
}
