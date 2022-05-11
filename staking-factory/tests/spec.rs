use near_sdk::json_types::Base58CryptoHash;
use near_sdk::serde_json::{self, json};
use near_sdk::{AccountId, PublicKey};
use near_sdk_sim::{
    call, deploy, init_simulator, to_yocto, view, ContractAccount, ExecutionResult, UserAccount,
};

use near_sdk_sim::transaction::ExecutionStatus;
use staking_factory::{Ratio, StakingPoolFactoryContract};

const STAKING_POOL_WHITELIST_ACCOUNT_ID: &str = "staking-pool-whitelist";
const STAKING_POOL_ID: &str = "pool";
const STAKING_POOL_ACCOUNT_ID: &str = "pool.factory";
const STAKING_KEY: &str = "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7";
const POOL_DEPOSIT: &str = "50";

near_sdk_sim::lazy_static_include::lazy_static_include_bytes! {
    FACTORY_WASM_BYTES => "../res/staking_factory_release.wasm",
    WHITELIST_WASM_BYTES => "../res/whitelist.wasm",
    STAKING_FARM_1_0_0_BYTES => "../res/staking_farm_release_1.0.0.wasm",
    STAKING_FARM_BYTES => "../res/staking_farm_release.wasm",
}

type FactoryContract = ContractAccount<StakingPoolFactoryContract>;

fn whitelist_id() -> AccountId {
    AccountId::new_unchecked(STAKING_POOL_WHITELIST_ACCOUNT_ID.to_string())
}

fn setup_factory() -> (UserAccount, UserAccount, FactoryContract, Base58CryptoHash) {
    let root = init_simulator(None);
    let foundation = root.create_user(
        AccountId::new_unchecked("foundation".to_string()),
        to_yocto("100"),
    );

    root.deploy_and_init(
        &WHITELIST_WASM_BYTES,
        whitelist_id(),
        "new",
        &serde_json::to_string(&json!({ "foundation_account_id": foundation.account_id() }))
            .unwrap()
            .as_bytes(),
        to_yocto("5"),
        near_sdk_sim::DEFAULT_GAS,
    );
    let factory = deploy!(
        contract: StakingPoolFactoryContract,
        contract_id: "factory".to_string(),
        bytes: &FACTORY_WASM_BYTES,
        signer_account: root,
        deposit: to_yocto("200"),
        init_method: new(foundation.account_id.clone(), whitelist_id())
    );
    foundation
        .call(
            whitelist_id(),
            "add_factory",
            &serde_json::to_vec(&json!({"factory_account_id": "factory".to_string()})).unwrap(),
            near_sdk_sim::DEFAULT_GAS,
            0,
        )
        .assert_success();
    let hash = foundation
        .call(
            factory.account_id(),
            "store",
            &STAKING_FARM_BYTES,
            near_sdk_sim::DEFAULT_GAS,
            to_yocto("5"),
        )
        .unwrap_json::<Base58CryptoHash>();
    call!(foundation, factory.allow_contract(hash)).assert_success();
    (root, foundation, factory, hash)
}

fn is_whitelisted(account: &UserAccount, account_id: &str) -> bool {
    account
        .view(
            whitelist_id(),
            "is_whitelisted",
            &serde_json::to_string(&json!({ "staking_pool_account_id": account_id }))
                .unwrap()
                .as_bytes(),
        )
        .unwrap_json::<bool>()
}

fn create_staking_pool(
    user: &UserAccount,
    factory: &FactoryContract,
    code_hash: Base58CryptoHash,
) -> ExecutionResult {
    let fee = Ratio {
        numerator: 10,
        denominator: 100,
    };
    call!(
        user,
        factory.create_staking_pool(
            STAKING_POOL_ID.to_string(),
            code_hash,
            user.account_id(),
            STAKING_KEY.parse().unwrap(),
            fee
        ),
        deposit = to_yocto(POOL_DEPOSIT)
    )
}

pub fn should_fail(r: ExecutionResult) {
    match r.status() {
        ExecutionStatus::Failure(_) => {}
        _ => panic!("Should fail"),
    }
}

fn assert_all_success(result: ExecutionResult) {
    let mut all_success = true;
    let mut all_results = String::new();
    for r in result.promise_results() {
        let x = r.expect("NO_RESULT");
        all_results = format!("{}\n{:?}", all_results, x);
        all_success &= x.is_ok();
    }
    println!("{}", all_results);
    assert!(
        all_success,
        "Not all promises where successful: \n\n{}",
        all_results
    );
}

fn get_staking_pool_key(user: &UserAccount) -> PublicKey {
    user.view(
        AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
        "get_staking_key",
        &[],
    )
    .unwrap_json()
}

fn get_version(user: &UserAccount) -> String {
    user.view(
        AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
        "get_version",
        &[],
    )
    .unwrap_json()
}

#[test]
fn create_staking_pool_success() {
    let (root, _foundation, factory, code_hash) = setup_factory();
    let balance = to_yocto("100");
    let user1 = root.create_user(AccountId::new_unchecked("user1".to_string()), balance);
    assert_all_success(create_staking_pool(&user1, &factory, code_hash));
    let pools_created = view!(factory.get_number_of_staking_pools_created()).unwrap_json::<u64>();
    assert_eq!(pools_created, 1);
    assert!(is_whitelisted(&root, STAKING_POOL_ACCOUNT_ID));

    // The caller was charged the amount + some for fees
    let new_balance = user1.account().unwrap().amount;
    assert!(new_balance > balance - to_yocto(POOL_DEPOSIT) - to_yocto("0.02"));

    // Pool account was created and attached deposit was transferred + some from 30% dev fees.
    let acc = root
        .borrow_runtime()
        .view_account(STAKING_POOL_ACCOUNT_ID)
        .expect("MUST BE CREATED");
    assert!(acc.amount + acc.locked > to_yocto(POOL_DEPOSIT));

    // The staking key on the pool matches the one that was given.
    assert_eq!(get_staking_pool_key(&root), STAKING_KEY.parse().unwrap());
}

fn wait_epoch(user: &UserAccount) {
    let epoch_height = user.borrow_runtime().cur_block.epoch_height;
    while user.borrow_runtime().cur_block.epoch_height == epoch_height {
        assert!(user.borrow_runtime_mut().produce_block().is_ok());
    }
}

#[test]
fn test_staking_pool_burn() {
    let (root, _foundation, factory, code_hash) = setup_factory();
    create_staking_pool(&root, &factory, code_hash).assert_success();
    let account_id = AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string());
    assert_all_success(root.call(
        account_id.clone(),
        "deposit_and_stake",
        &[],
        near_sdk_sim::DEFAULT_GAS,
        to_yocto("100000000"),
    ));
    wait_epoch(&root);
    assert_all_success(root.call(account_id, "ping", &[], near_sdk_sim::DEFAULT_GAS, 0));
}

#[test]
fn test_get_code() {
    let (_root, _foundation, factory, code_hash) = setup_factory();
    let result: Vec<u8> = view!(factory.get_code(code_hash)).unwrap();
    assert_eq!(result, STAKING_FARM_BYTES.to_vec());
    assert!(view!(factory.get_code([0u8; 32].into()))
        .unwrap_err()
        .to_string()
        .find("Contract hash is not allowed")
        .is_some());
}

#[test]
fn test_staking_pool_upgrade_from_1_0_0() {
    let (root, foundation, factory, code_hash) = setup_factory();
    let hash_1_0_0 = foundation
        .call(
            factory.account_id(),
            "store",
            &STAKING_FARM_1_0_0_BYTES,
            near_sdk_sim::DEFAULT_GAS,
            to_yocto("5"),
        )
        .unwrap_json::<Base58CryptoHash>();
    call!(foundation, factory.allow_contract(hash_1_0_0)).assert_success();

    create_staking_pool(&root, &factory, hash_1_0_0).assert_success();

    let attempted_get_version_view = root.view(
        AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
        "get_version",
        &[],
    );
    assert!(attempted_get_version_view.is_err());

    let version_through_call: String = root
        .call(
            AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
            "get_version",
            &[],
            near_sdk_sim::DEFAULT_GAS,
            0,
        )
        .unwrap_json();
    assert_eq!(version_through_call, "staking-farm:1.0.0");

    // Upgrade staking pool.
    assert_all_success(root.call(
        AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
        "upgrade",
        &serde_json::to_vec(&json!({ "code_hash": code_hash })).unwrap(),
        near_sdk_sim::DEFAULT_GAS,
        0,
    ));
    // Check that contract works.
    assert_eq!(get_staking_pool_key(&root), STAKING_KEY.parse().unwrap());
    assert_eq!(get_version(&root), "staking-farm:1.1.0");
}
