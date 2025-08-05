mod helpers;
mod pocket_ic_agent;

use candid::{Nat, Principal};
use helpers::Wasm;
use ic_icrc1_ledger::{InitArgsBuilder, LedgerArgument};
use ic_management_canister_types::CanisterSettings;
use icp_ledger::{AccountIdentifier, LedgerCanisterInitPayload};
use icrc_ledger_types::{
    icrc1::{account::Account, transfer::TransferArg},
    icrc2::{approve::ApproveArgs, transfer_from::TransferFromArgs},
};
use kongswap_adaptor::agent::AbstractAgent;
use lazy_static::lazy_static;
use pocket_ic::{nonblocking::PocketIc, PocketIcBuilder};
use pocket_ic_agent::PocketIcAgent;
// use pretty_assertions::assert_eq;
use sha2::Digest;
use std::time::Duration;

pub const STARTING_CYCLES_PER_CANISTER: u128 = 2_000_000_000_000_000;

const E8: u64 = 100_000_000;
const NS_IN_SECOND: u64 = 1_000_000_000;
const ONE_HOUR: u64 = 60 * 60 * NS_IN_SECOND;

lazy_static! {
    static ref SNS_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("jg2ra-syaaa-aaaaq-aaewa-cai").unwrap();
    static ref SNS_ROOT_CANISTER_ID: Principal =
        Principal::from_text("ju4gz-6iaaa-aaaaq-aaeva-cai").unwrap();
    static ref NNS_ROOT_CANISTER_ID: Principal =
        Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
    static ref SNS_GOVERNANCE_CANISTER_ID: Principal =
        Principal::from_text("jt5an-tqaaa-aaaaq-aaevq-cai").unwrap();
    static ref ICP_LEDGER_CANISTER_ID: Principal =
        Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
    static ref NNS_GOVERNANCE_CANISTER_ID: Principal =
        Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
}

#[tokio::test]
async fn allowance_expiry() {
    // Prepare the world.

    let pocket_ic = PocketIcBuilder::new()
        .with_nns_subnet()
        .with_sns_subnet()
        .with_fiduciary_subnet()
        .build_async()
        .await;

    let mut agent = PocketIcAgent::new(pocket_ic);

    let sns_ledger_canister_ic = install_sns_ledger(&agent.pic()).await;
    let icp_ledger_canister_id = install_icp_ledger(&agent.pic()).await;

    let owner = Principal::from_text("uz6mw-eaaaa-aaaaq-aabpa-cai").unwrap();
    let reciever = Principal::from_text("4bli7-7iaaa-aaaap-ahd4a-cai").unwrap();

    mint_tokens(
        agent.with_sender(*SNS_ROOT_CANISTER_ID),
        sns_ledger_canister_ic,
        Account {
            owner,
            subaccount: None,
        },
        100 * E8,
    )
    .await;

    mint_tokens(
        agent.with_sender(*NNS_GOVERNANCE_CANISTER_ID),
        icp_ledger_canister_id,
        Account {
            owner,
            subaccount: None,
        },
        100 * E8,
    )
    .await;

    let expires_at = agent.pic().get_time().await.as_nanos_since_unix_epoch() + ONE_HOUR;
    give_allowance(
        agent.with_sender(owner),
        &reciever,
        expires_at,
        sns_ledger_canister_ic,
        50 * E8,
    )
    .await;

    give_allowance(
        agent.with_sender(owner),
        &reciever,
        expires_at,
        icp_ledger_canister_id,
        50 * E8,
    )
    .await;

    // In less than 1 hour, transfer from should not fail
    transfer_from(
        agent.with_sender(reciever),
        &owner,
        &reciever,
        sns_ledger_canister_ic,
        10 * E8,
        false,
    )
    .await;

    transfer_from(
        agent.with_sender(reciever),
        &owner,
        &reciever,
        icp_ledger_canister_id,
        10 * E8,
        false,
    )
    .await;

    // We need between 50 and 100 ticks to get the initial deposit and the first batch of periodic
    // tasks to be processed.
    agent
        .pic()
        .advance_time(Duration::from_nanos(ONE_HOUR))
        .await;
    agent.pic().tick().await;

    // Later than 1 hour, transfer from should fail
    transfer_from(
        agent.with_sender(reciever),
        &owner,
        &reciever,
        sns_ledger_canister_ic,
        10 * E8,
        true,
    )
    .await;

    transfer_from(
        agent.with_sender(reciever),
        &owner,
        &reciever,
        icp_ledger_canister_id,
        10 * E8,
        true,
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

async fn give_allowance<Agent>(
    agent: &mut Agent,
    owner: &Principal,
    expires_at: u64,
    icrc1_ledger_canister_id: Principal,
    amount: u64,
) where
    Agent: AbstractAgent,
{
    let request = ApproveArgs {
        from_subaccount: None,
        spender: Account {
            owner: *owner,
            subaccount: None,
        },
        // All approved tokens should be fully used up before the next deposit.
        amount: Nat::from(amount),
        expected_allowance: Some(Nat::from(0u8)),
        expires_at: Some(expires_at),
        memo: None,
        created_at_time: None,
        fee: None,
    };

    let _response = agent
        .call(icrc1_ledger_canister_id, request)
        .await
        .unwrap()
        .unwrap();
}

async fn transfer_from<Agent>(
    agent: &mut Agent,
    from_principal_id: &Principal,
    to_principal_id: &Principal,
    icrc1_ledger_canister_id: Principal,
    amount: u64,
    should_fail: bool,
) -> bool
where
    Agent: AbstractAgent,
{
    let request = TransferFromArgs {
        spender_subaccount: None,
        from: Account {
            owner: *from_principal_id,
            subaccount: None,
        },
        to: Account {
            owner: *to_principal_id,
            subaccount: None,
        },
        amount: Nat::from(amount),
        fee: None,
        memo: None,
        created_at_time: None,
    };

    let response = agent.call(icrc1_ledger_canister_id, request).await.unwrap();

    response.is_err() == should_fail
}
