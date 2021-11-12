use near_sdk::json_types::Base58CryptoHash;
use near_sdk::serde_json::{self, json};
use near_sdk::{AccountId, PublicKey};
use near_sdk_sim::{call, deploy, init_simulator, to_yocto, view, ContractAccount, UserAccount};

use staking_factory::{RewardFeeFraction, StakingPoolFactoryContract};

const STAKING_POOL_WHITELIST_ACCOUNT_ID: &str = "staking-pool-whitelist";
const STAKING_POOL_ID: &str = "pool";
const STAKING_POOL_ACCOUNT_ID: &str = "pool.factory";

near_sdk_sim::lazy_static_include::lazy_static_include_bytes! {
    FACTORY_WASM_BYTES => "../res/staking_factory_local.wasm",
    WHITELIST_WASM_BYTES => "../res/whitelist.wasm",
    STAKING_FARM_BYTES => "../res/staking_farm_local.wasm",
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

#[test]
fn create_staking_pool_success() {
    let (root, _foundation, factory, code_hash) = setup_factory();
    let balance = to_yocto("100");
    let pool_deposit = to_yocto("50");
    let user1 = root.create_user(AccountId::new_unchecked("user1".to_string()), balance);
    let fee = RewardFeeFraction {
        numerator: 10,
        denominator: 100,
    };
    let staking_key: PublicKey = "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
        .parse()
        .unwrap();
    let result = call!(
        user1,
        factory.create_staking_pool(
            STAKING_POOL_ID.to_string(),
            code_hash,
            user1.account_id(),
            staking_key.clone(),
            fee
        ),
        deposit = pool_deposit
    );
    result.assert_success();
    println!("{:?}", result.promise_results());
    let pools_created = view!(factory.get_number_of_staking_pools_created()).unwrap_json::<u64>();
    assert_eq!(pools_created, 1);
    assert!(is_whitelisted(&root, STAKING_POOL_ACCOUNT_ID));

    // The caller was charged the amount + some for fees
    let new_balance = user1.account().unwrap().amount;
    assert!(new_balance > balance - pool_deposit - to_yocto("0.02"));

    // Pool account was created and attached deposit was transferred + some from 30% dev fees.
    let acc = root
        .borrow_runtime()
        .view_account(STAKING_POOL_ACCOUNT_ID)
        .expect("MUST BE CREATED");
    assert!(acc.amount + acc.locked > pool_deposit);

    // The staking key on the pool matches the one that was given.
    let actual_staking_key: PublicKey = root
        .view(
            AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
            "get_staking_key",
            &[],
        )
        .unwrap_json();
    assert_eq!(actual_staking_key, staking_key);
}
