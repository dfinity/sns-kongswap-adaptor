mod helpers;
mod pocket_ic_agent;

use candid::{Nat, Principal};
use helpers::Wasm;
use ic_icrc1_ledger::{InitArgsBuilder, LedgerArgument};
use ic_management_canister_types::CanisterSettings;
use icp_ledger::{AccountIdentifier, LedgerCanisterInitPayload};
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg};
use kongswap_adaptor::{agent::AbstractAgent, audit::serialize_audit_trail};
use lazy_static::lazy_static;
use pocket_ic::{nonblocking::PocketIc, PocketIcBuilder};
use pocket_ic_agent::PocketIcAgent;
use sha2::Digest;
use sns_treasury_manager::{
    self, Allowance, Asset, AuditTrailRequest, BalancesRequest, TreasuryManagerArg,
    TreasuryManagerInit, TreasuryManagerUpgrade,
};
use std::time::Duration;

pub const STARTING_CYCLES_PER_CANISTER: u128 = 2_000_000_000_000_000;

const FEE: u64 = 10_000;
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
}

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
    let sns_ledger_canister_ic = install_sns_ledger(&agent.pic()).await;
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

    // We need fewer than 50 ticks to get the initial deposit to be processed.
    for _ in 0..50 {
        agent.pic().advance_time(Duration::from_secs(1)).await;
        agent.pic().tick().await;
    }

    let balances_before_upgrade = agent
        .call(kong_adaptor_canister_id, BalancesRequest {})
        .await
        .unwrap()
        .unwrap();

    let audit_trail_before_upgrade = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
        .unwrap();

    let module_hash_before_upgrade = agent
        .pic()
        .canister_status(kong_adaptor_canister_id, Some(*SNS_ROOT_CANISTER_ID))
        .await
        .unwrap()
        .module_hash
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

    let audit_trail_after_upgrade = agent
        .call(kong_adaptor_canister_id, AuditTrailRequest {})
        .await
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
    assert_eq!(
        audit_trail_after_second_upgrade,
        audit_trail_after_second_upgrade
    );

    let _audit_trail = serialize_audit_trail(&audit_trail_after_second_upgrade).unwrap();
    // println!("Audit Trail: {audit_trail}");
    // panic!("The end");
}

async fn create_kong_adaptor(pocket_ic: &PocketIc, subnet_id: Principal) -> Principal {
    let controllers = vec![*SNS_ROOT_CANISTER_ID];

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
    let sns_asset = Asset::Token {
        symbol: "SNS".to_string(),
        ledger_canister_id: *SNS_LEDGER_CANISTER_ID,
        ledger_fee_decimals: Nat::from(FEE),
    };

    let icp_asset = Asset::Token {
        symbol: "ICP".to_string(),
        ledger_canister_id: *ICP_LEDGER_CANISTER_ID,
        ledger_fee_decimals: Nat::from(FEE),
    };

    let arg = TreasuryManagerArg::Init(TreasuryManagerInit {
        allowances: vec![
            Allowance {
                asset: sns_asset,
                amount_decimals: Nat::from(amount_sns_e8s),
                owner_account: treasury_icp_account,
            },
            Allowance {
                amount_decimals: Nat::from(amount_icp_e8s),
                asset: icp_asset,
                owner_account: treasury_sns_account,
            },
        ],
    });

    let arg = candid::encode_one(&arg).unwrap();

    pocket_ic
        .install_canister(canister_id, wasm.bytes(), arg, Some(*SNS_ROOT_CANISTER_ID))
        .await;

    let subnet_id = pocket_ic.get_subnet(canister_id).await.unwrap();
    println!(
        "Installed the KongSwapAdaptor canister ({}) onto subnet {}",
        canister_id, subnet_id
    );
}

async fn install_kong_swap(pocket_ic: &PocketIc) -> Principal {
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

    canister_id
}

async fn mint_tokens<Agent>(
    agent: &Agent,
    icrc1_ledger_canister_id: Principal,
    beneficiary_account: Account,
    amount_e8s: u64,
) where
    Agent: AbstractAgent,
{
    let request = TransferArg {
        from_subaccount: None,
        to: beneficiary_account,
        fee: Some(Nat::from(0_u8)),
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

    let owner = *SNS_ROOT_CANISTER_ID;
    let controllers = vec![owner];

    let arg = InitArgsBuilder::with_symbol_and_name("SNS", "My DAO Token")
        .with_minting_account(owner)
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
