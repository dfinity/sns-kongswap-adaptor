mod helpers;
mod kongswap_types;
mod pocket_ic_agent;

use candid::{Nat, Principal};
use helpers::Wasm;
use ic_icrc1_ledger::{InitArgsBuilder, LedgerArgument};
use ic_management_canister_types::CanisterSettings;
use icp_ledger::{AccountIdentifier, LedgerCanisterInitPayload, DEFAULT_TRANSFER_FEE};
use icrc_ledger_types::{
    icrc1::{account::Account, transfer::TransferArg},
    icrc2::approve::ApproveArgs,
};
use kongswap_adaptor::agent::AbstractAgent;
use kongswap_types::SwapArgs;
use lazy_static::lazy_static;
use pocket_ic::{nonblocking::PocketIc, PocketIcBuilder};
use pocket_ic_agent::PocketIcAgent;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sns_treasury_manager::{
    self, Allowance, Asset, BalanceBook, BalancesRequest, DepositRequest, TreasuryManagerArg,
    TreasuryManagerInit, WithdrawRequest,
};
use std::time::Duration;

pub const STARTING_CYCLES_PER_CANISTER: u128 = 2_000_000_000_000_000;

const FEE_ICP: u64 = 10_000;
const FEE_SNS: u64 = 10_000;
const E8: u64 = 100_000_000;

lazy_static! {
    static ref SNS_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("jg2ra-syaaa-aaaaq-aaewa-cai").unwrap();

    static ref SNS_ROOT_CANISTER_ID: Principal =
        Principal::from_text("ju4gz-6iaaa-aaaaq-aaeva-cai").unwrap();

    static ref SNS_GOVERNANCE_CANISTER_ID: Principal =
        Principal::from_text("jt5an-tqaaa-aaaaq-aaevq-cai").unwrap();

    static ref ICP_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();

    static ref NNS_ROOT_CANISTER_ID: Principal =
        Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();

    static ref NNS_GOVERNANCE_CANISTER_ID: Principal =
        Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();

    // Canister ID from the mainnet.
    // See https://dashboard.internetcomputer.org/canister/2ipq2-uqaaa-aaaar-qailq-cai
    static ref KONGSWAP_BACKEND_CANISTER_ID: Principal =
        Principal::from_text("2ipq2-uqaaa-aaaar-qailq-cai").unwrap();

    static ref TRADER_PRINCIPAL_ID: Principal = Principal::from_text("zmbi7-fyaaa-aaaaq-aaahq-cai").unwrap();

    static ref SNS_ASSET: Asset = Asset::Token {
        symbol: "SNS".to_string(),
        ledger_canister_id: *SNS_LEDGER_CANISTER_ID,
        ledger_fee_decimals: Nat::from(FEE_SNS),
    };

    static ref ICP_ASSET: Asset = Asset::Token {
        symbol: "ICP".to_string(),
        ledger_canister_id: *ICP_LEDGER_CANISTER_ID,
        ledger_fee_decimals: Nat::from(FEE_ICP),
    };

}

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

    // Phase I: setup the canister
    setup_kongswap_adaptor(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        &original_wasm,
        treasury_icp_account,
        treasury_sns_account,
    )
    .await;

    // Phase II: another deposit
    deposit(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        treasury_sns_account,
        treasury_icp_account,
        10 * E8,
    )
    .await;
    // Phase III: first trade
    trade(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        1 * E8,
        true,
    )
    .await;
    // Phase IV: another deposit
    deposit(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        kong_adaptor_canister_id,
        treasury_sns_account,
        treasury_icp_account,
        10 * E8,
    )
    .await;
    // Phase V: second trade
    trade(
        &mut agent,
        sns_ledger_canister_id,
        icp_ledger_canister_id,
        1 * E8,
        false,
    )
    .await;

    // At this point pool has more SNS than ICP
    {
        let (balances_0, balances_1) = get_balances(&mut agent, kong_adaptor_canister_id).await;

        assert!(
            balances_0
                .external_custodian
                .as_ref()
                .unwrap()
                .amount_decimals
                > balances_1
                    .external_custodian
                    .as_ref()
                    .unwrap()
                    .amount_decimals,
            "We expect to have more SNS in the pool than ICP"
        );
    }
    // VI: final phase: withdrawal
    {
        let (balances_0, balances_1) = withdraw(&mut agent, kong_adaptor_canister_id).await;
        assert_eq!(
            balances_0.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(8 * FEE_SNS)
        );
        assert_eq!(
            balances_1.fee_collector.as_ref().unwrap().amount_decimals,
            Nat::from(9 * FEE_ICP)
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
    }
}

async fn setup_kongswap_adaptor(
    agent: &mut PocketIcAgent,
    sns_ledger_canister_id: Principal,
    icp_ledger_canister_id: Principal,
    kong_adaptor_canister_id: Principal,
    wasm: &Wasm,
    treasury_icp_account: sns_treasury_manager::Account,
    treasury_sns_account: sns_treasury_manager::Account,
) {
    let initial_deposit = 100 * E8;
    mint_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        initial_deposit,
    )
    .await;

    mint_tokens(
        agent.with_sender(*NNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        initial_deposit,
    )
    .await;

    install_kong_adaptor(
        &agent.pic(),
        wasm.clone(),
        kong_adaptor_canister_id,
        treasury_icp_account,
        treasury_sns_account,
        initial_deposit,
        initial_deposit,
    )
    .await;

    // We need between 50 and 100 ticks to get the initial deposit and the first batch of periodic
    // tasks to be processed.
    for _ in 0..100 {
        agent.pic().advance_time(Duration::from_secs(1)).await;
        agent.pic().tick().await;
    }
}

async fn deposit(
    agent: &mut PocketIcAgent,
    sns_ledger_canister_id: Principal,
    icp_ledger_canister_id: Principal,
    kong_adaptor_canister_id: Principal,
    treasury_sns_account: sns_treasury_manager::Account,
    treasury_icp_account: sns_treasury_manager::Account,
    topup: u64,
) {
    // let topup = 10 * E8;
    mint_tokens(
        agent.with_sender(*SNS_GOVERNANCE_CANISTER_ID),
        sns_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        topup,
    )
    .await;

    mint_tokens(
        agent.with_sender(*NNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner: kong_adaptor_canister_id,
            subaccount: None,
        },
        topup,
    )
    .await;

    let deposit_request = DepositRequest {
        allowances: vec![
            Allowance {
                asset: SNS_ASSET.clone(),
                amount_decimals: Nat::from(topup),
                owner_account: treasury_sns_account,
            },
            Allowance {
                amount_decimals: Nat::from(topup),
                asset: ICP_ASSET.clone(),
                owner_account: treasury_icp_account,
            },
        ],
    };

    let _deposit_response = agent
        .with_sender(*SNS_GOVERNANCE_CANISTER_ID)
        .call(kong_adaptor_canister_id, deposit_request)
        .await
        .unwrap()
        .unwrap();

    for _ in 0..10 {
        agent.pic().advance_time(Duration::from_secs(1)).await;
        agent.pic().tick().await;
    }
}

async fn trade(
    agent: &mut PocketIcAgent,
    sns_ledger_canister_id: Principal,
    icp_ledger_canister_id: Principal,
    trader_balance: u64,
    trade_sns: bool,
) {
    let (fee, ledger_canister_id, pay_token, receive_token, minter_account) = if trade_sns {
        (
            FEE_SNS,
            sns_ledger_canister_id,
            format!("IC.{}", sns_ledger_canister_id),
            format!("IC.{}", icp_ledger_canister_id),
            *SNS_GOVERNANCE_CANISTER_ID,
        )
    } else {
        (
            FEE_ICP,
            icp_ledger_canister_id,
            format!("IC.{}", icp_ledger_canister_id),
            format!("IC.{}", sns_ledger_canister_id),
            *NNS_GOVERNANCE_CANISTER_ID,
        )
    };

    mint_tokens(
        agent.with_sender(minter_account),
        ledger_canister_id,
        Account {
            owner: TRADER_PRINCIPAL_ID.clone(),
            subaccount: None,
        },
        trader_balance,
    )
    .await;

    // First we give allowance to the Kongswap backend
    let approve_request = ApproveArgs {
        from_subaccount: None,
        spender: Account {
            owner: *KONGSWAP_BACKEND_CANISTER_ID,
            subaccount: None,
        },
        amount: Nat::from(trader_balance - fee),
        expected_allowance: Some(Nat::from(0u8)),
        expires_at: Some(u64::MAX),
        memo: None,
        created_at_time: None,
        fee: Some(Nat::from(fee)),
    };

    let _approve_args_response = agent
        .with_sender(TRADER_PRINCIPAL_ID.clone())
        .call(ledger_canister_id, approve_request)
        .await
        .unwrap()
        .unwrap();

    let swap_request = SwapArgs {
        pay_token,
        pay_amount: Nat::from(trader_balance - 2 * fee),
        pay_tx_id: None,
        receive_token,
        receive_amount: None,
        receive_address: None,
        max_slippage: Some(10.0),
        referred_by: None,
    };

    let _response = agent
        .with_sender(TRADER_PRINCIPAL_ID.clone())
        .call(*KONGSWAP_BACKEND_CANISTER_ID, swap_request)
        .await
        .unwrap()
        .unwrap();

    for _ in 0..10 {
        agent.pic().advance_time(Duration::from_secs(1)).await;
        agent.pic().tick().await;
    }
}

async fn get_balances(
    agent: &mut PocketIcAgent,
    kong_adaptor_canister_id: Principal,
) -> (BalanceBook, BalanceBook) {
    let balances_request = BalancesRequest {};
    let balances = agent
        .call(kong_adaptor_canister_id, balances_request)
        .await
        .unwrap()
        .unwrap();

    let asset_to_balance = balances.asset_to_balances.unwrap();
    let balances_0 = asset_to_balance.get(&SNS_ASSET).unwrap();
    let balances_1 = asset_to_balance.get(&ICP_ASSET).unwrap();

    (balances_0.clone(), balances_1.clone())
}

async fn withdraw(
    agent: &mut PocketIcAgent,
    kong_adaptor_canister_id: Principal,
) -> (BalanceBook, BalanceBook) {
    let withdraw_request = WithdrawRequest {
        withdraw_accounts: None,
    };

    let response = agent
        .with_sender(*SNS_GOVERNANCE_CANISTER_ID)
        .call(kong_adaptor_canister_id, withdraw_request)
        .await
        .unwrap()
        .unwrap();

    let asset_to_balance = response.asset_to_balances.unwrap();
    let balances_0 = asset_to_balance.get(&SNS_ASSET).unwrap();
    let balances_1 = asset_to_balance.get(&ICP_ASSET).unwrap();

    (balances_0.clone(), balances_1.clone())
}

async fn create_kong_adaptor(pocket_ic: &PocketIc, subnet_id: Principal) -> Principal {
    let controllers = vec![*SNS_ROOT_CANISTER_ID, *SNS_GOVERNANCE_CANISTER_ID];

    let (canister_id, _) = create_canister_with_controllers(
        pocket_ic,
        CanisterInstallationTarget::SubnetId(subnet_id),
        controllers,
    )
    .await;

    canister_id
}

fn get_kong_adaptor_wasm() -> Wasm {
    let wasm_path = std::env::var("KONGSWAP_ADAPTOR_CANISTER_WASM_PATH")
        .expect("KONGSWAP_ADAPTOR_CANISTER_WASM_PATH must be set.");
    Wasm::from_file(wasm_path)
}

async fn install_kong_adaptor(
    pocket_ic: &PocketIc,
    wasm: Wasm,
    canister_id: Principal,
    treasury_icp_account: sns_treasury_manager::Account,
    treasury_sns_account: sns_treasury_manager::Account,
    amount_icp_e8s: u64,
    amount_sns_e8s: u64,
) {
    let arg = TreasuryManagerArg::Init(TreasuryManagerInit {
        allowances: vec![
            Allowance {
                asset: SNS_ASSET.clone(),
                amount_decimals: Nat::from(amount_sns_e8s),
                owner_account: treasury_sns_account,
            },
            Allowance {
                amount_decimals: Nat::from(amount_icp_e8s),
                asset: ICP_ASSET.clone(),
                owner_account: treasury_icp_account,
            },
        ],
    });

    let arg = candid::encode_one(&arg).unwrap();

    pocket_ic
        .install_canister(
            canister_id,
            wasm.bytes(),
            arg,
            Some(*SNS_GOVERNANCE_CANISTER_ID),
        )
        .await;

    let subnet_id = pocket_ic.get_subnet(canister_id).await.unwrap();
    println!(
        "Installed the KongSwapAdaptor canister ({}) onto subnet {}",
        canister_id, subnet_id
    );
}

async fn install_kong_swap(pocket_ic: &PocketIc) {
    // Install KongSwap
    let wasm_path = std::env::var("KONG_BACKEND_CANISTER_WASM_PATH")
        .expect("KONG_BACKEND_CANISTER_WASM_PATH must be set.");

    let kong_backend_wasm = Wasm::from_file(wasm_path);

    let controllers = vec![Principal::anonymous()];

    let canister_id = *KONGSWAP_BACKEND_CANISTER_ID;

    install_canister_with_controllers(
        pocket_ic,
        "KongSwap Backend Canister",
        CanisterInstallationTarget::CanisterId(canister_id),
        vec![],
        kong_backend_wasm,
        controllers,
    )
    .await;
}

async fn mint_tokens<Agent>(
    agent: &mut Agent,
    icrc1_ledger_canister_id: Principal,
    beneficiary_account: Account,
    amount_e8s: u64,
) where
    Agent: AbstractAgent,
{
    let request = TransferArg {
        from_subaccount: None,
        to: beneficiary_account,
        fee: None,
        created_at_time: None,
        memo: None,
        amount: Nat::from(amount_e8s),
    };

    let _response = agent
        .call(icrc1_ledger_canister_id, request)
        .await
        .unwrap()
        .unwrap();
}

async fn install_sns_ledger(pocket_ic: &PocketIc) -> Principal {
    let wasm_path =
        std::env::var("IC_ICRC1_LEDGER_WASM_PATH").expect("IC_ICRC1_LEDGER_WASM_PATH must be set.");

    let icrc1_wasm = Wasm::from_file(wasm_path);

    let controllers = vec![*SNS_ROOT_CANISTER_ID];

    let arg = InitArgsBuilder::with_symbol_and_name("SNS", "My DAO Token")
        .with_transfer_fee(FEE_SNS)
        .with_minting_account(*SNS_GOVERNANCE_CANISTER_ID)
        .build();

    let arg = LedgerArgument::Init(arg);

    let arg = candid::encode_one(&arg).unwrap();

    let canister_id = *SNS_LEDGER_CANISTER_ID;

    install_canister_with_controllers(
        &pocket_ic,
        "SNS Ledger",
        CanisterInstallationTarget::CanisterId(canister_id),
        arg,
        icrc1_wasm,
        controllers,
    )
    .await;

    canister_id
}

async fn install_icp_ledger(pocket_ic: &PocketIc) -> Principal {
    let wasm_path = std::env::var("MAINNET_ICP_LEDGER_CANISTER_WASM_PATH")
        .expect("MAINNET_ICP_LEDGER_CANISTER_WASM_PATH must be set.");

    let icp_ledger_wasm = Wasm::from_file(wasm_path);

    let controllers = vec![*NNS_ROOT_CANISTER_ID];

    let arg = LedgerCanisterInitPayload::builder()
        .transfer_fee(DEFAULT_TRANSFER_FEE)
        .minting_account(AccountIdentifier::from(*NNS_GOVERNANCE_CANISTER_ID))
        .build()
        .unwrap();

    let arg = candid::encode_one(&arg).unwrap();

    let canister_id = *ICP_LEDGER_CANISTER_ID;

    install_canister_with_controllers(
        &pocket_ic,
        "ICP Ledger",
        CanisterInstallationTarget::CanisterId(canister_id),
        arg,
        icp_ledger_wasm,
        controllers,
    )
    .await;

    canister_id
}

pub enum CanisterInstallationTarget {
    CanisterId(Principal),
    SubnetId(Principal),
}

pub async fn create_canister_with_controllers(
    pocket_ic: &PocketIc,
    target: CanisterInstallationTarget,
    controllers: Vec<Principal>,
) -> (Principal, Option<Principal>) {
    let sender = controllers.first().cloned();
    let settings = Some(CanisterSettings {
        controllers: Some(controllers),
        ..Default::default()
    });

    let canister_id = match target {
        CanisterInstallationTarget::CanisterId(canister_id) => pocket_ic
            .create_canister_with_id(sender, settings, canister_id)
            .await
            .unwrap(),
        CanisterInstallationTarget::SubnetId(subnet_id) => {
            pocket_ic
                .create_canister_on_subnet(sender, settings, subnet_id)
                .await
        }
    };

    pocket_ic
        .add_cycles(canister_id, STARTING_CYCLES_PER_CANISTER)
        .await;

    (canister_id, sender)
}

pub async fn install_canister_with_controllers(
    pocket_ic: &PocketIc,
    name: &str,
    target: CanisterInstallationTarget,
    arg: Vec<u8>,
    wasm: Wasm,
    controllers: Vec<Principal>,
) -> Principal {
    let (canister_id, sender) =
        create_canister_with_controllers(pocket_ic, target, controllers).await;

    pocket_ic
        .install_canister(canister_id, wasm.bytes(), arg, sender)
        .await;
    let subnet_id = pocket_ic.get_subnet(canister_id).await.unwrap();
    println!(
        "Installed the {} canister ({}) onto {:?}",
        name, canister_id, subnet_id
    );

    canister_id
}

pub fn compute_treasury_subaccount_bytes(principal: Principal) -> [u8; 32] {
    /// The static MEMO used when calculating the SNS Treasury subaccount.
    const TREASURY_SUBACCOUNT_NONCE: u64 = 0;
    compute_distribution_subaccount_bytes(principal, TREASURY_SUBACCOUNT_NONCE)
}

/// Computes the subaccount to which locked token distributions are initialized to.
///
/// From ic/rs/nervous_system/common/src/ledger.rs
pub fn compute_distribution_subaccount_bytes(principal: Principal, nonce: u64) -> [u8; 32] {
    compute_neuron_domain_subaccount_bytes(principal, b"token-distribution", nonce)
}

/// From ic/rs/nervous_system/common/src/ledger.rs
fn compute_neuron_domain_subaccount_bytes(
    controller: Principal,
    domain: &[u8],
    nonce: u64,
) -> [u8; 32] {
    let domain_length: [u8; 1] = [domain.len() as u8];
    let mut hasher = sha2::Sha256::default();
    hasher.update(&domain_length);
    hasher.update(domain);
    hasher.update(controller.as_slice());
    hasher.update(&nonce.to_be_bytes());
    hasher.finalize().into()
}
