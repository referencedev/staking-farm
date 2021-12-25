use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::AccountId;
use near_sdk_sim::types::Balance;
use near_sdk_sim::{
    call, deploy, init_simulator, to_yocto, view, ContractAccount, ExecutionResult, UserAccount,
    ViewResult,
};

use near_sdk_sim::num_rational::Rational;
use staking_farm::{HumanReadableFarm, Ratio, StakingContractContract};

type PoolContract = ContractAccount<StakingContractContract>;

const STAKING_POOL_ACCOUNT_ID: &str = "pool";
const STAKING_KEY: &str = "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7";
const ONE_SEC_IN_NS: u64 = 1_000_000_000;
const WHITELIST_ACCOUNT_ID: &str = "whitelist";
const LOCKUP_ACCOUNT_ID: &str = "lockup";
const ZERO_ADDRESS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

near_sdk_sim::lazy_static_include::lazy_static_include_bytes! {
    STAKING_FARM_BYTES => "../res/staking_farm_local.wasm",
    TEST_TOKEN_BYTES => "../res/test_token.wasm",
    WHITELIST_BYTES => "../res/whitelist.wasm",
    LOCKUP_BYTES => "../res/lockup_contract.wasm",
}

fn token_id() -> AccountId {
    AccountId::new_unchecked("token".to_string())
}

fn lockup_id() -> AccountId {
    AccountId::new_unchecked(LOCKUP_ACCOUNT_ID.to_string())
}

fn burn_account() -> AccountId {
    AccountId::new_unchecked(ZERO_ADDRESS.to_string())
}

fn wait_epoch(user: &UserAccount) {
    let epoch_height = user.borrow_runtime().cur_block.epoch_height;
    while user.borrow_runtime().cur_block.epoch_height == epoch_height {
        assert!(user.borrow_runtime_mut().produce_block().is_ok());
    }
    // sim framework doesn't provide block rewards.
    // model the block reward by sending more funds on the account.
    user.transfer(
        AccountId::new_unchecked(STAKING_POOL_ACCOUNT_ID.to_string()),
        to_yocto("1000"),
    );
}

fn assert_all_success(result: ExecutionResult) {
    let mut all_success = true;
    let mut all_results = String::new();
    for r in result.promise_results() {
        let x = r.expect("NO_RESULT");
        all_results = format!("{}\n{:?}", all_results, x);
        all_success &= x.is_ok();
    }
    println!("{:?}", result.promise_results());
    assert!(
        all_success,
        "Not all promises where successful: \n\n{}",
        all_results
    );
}

fn call(
    user: &UserAccount,
    receiver_id: AccountId,
    method_name: &str,
    args: serde_json::Value,
    deposit: Balance,
) {
    assert_all_success(user.call(
        receiver_id,
        method_name,
        &serde_json::to_vec(&args).unwrap(),
        near_sdk_sim::DEFAULT_GAS,
        deposit,
    ));
}

fn storage_register(user: &UserAccount, account_id: AccountId) {
    call(
        user,
        token_id(),
        "storage_deposit",
        json!({ "account_id": account_id }),
        to_yocto("0.01"),
    );
}

fn setup(reward_ratio: u32, burn_ratio: u32) -> (UserAccount, PoolContract) {
    let root = init_simulator(None);
    // Disable contract rewards.
    root.borrow_runtime_mut()
        .genesis
        .runtime_config
        .transaction_costs
        .burnt_gas_reward = Rational::new(0, 1);
    let whitelist = root.deploy_and_init(
        &WHITELIST_BYTES,
        AccountId::new_unchecked(WHITELIST_ACCOUNT_ID.to_string()),
        "new",
        &serde_json::to_vec(&json!({ "foundation_account_id": root.account_id() })).unwrap(),
        to_yocto("10"),
        near_sdk_sim::DEFAULT_GAS,
    );
    let _other_token = root.deploy_and_init(
        &TEST_TOKEN_BYTES,
        token_id(),
        "new",
        &[],
        to_yocto("10"),
        near_sdk_sim::DEFAULT_GAS,
    );
    assert_all_success(root.call(
        token_id(),
        "mint",
        &serde_json::to_vec(&json!({ "account_id": root.account_id(), "amount": to_yocto("100000").to_string() })).unwrap(),
        near_sdk_sim::DEFAULT_GAS,
        0,
    ));
    let _lockup = root.deploy_and_init(
        &LOCKUP_BYTES,
        lockup_id(),
        "new",
        &serde_json::to_vec(&json!({ "owner_account_id": root.account_id(), "lockup_duration": "100000000000000", "transfers_information": { "TransfersEnabled": { "transfers_timestamp": "0" } }, "staking_pool_whitelist_account_id": WHITELIST_ACCOUNT_ID })).unwrap(),
        to_yocto("100000"),
        near_sdk_sim::DEFAULT_GAS,
    );
    let reward_ratio = Ratio {
        numerator: reward_ratio,
        denominator: 10,
    };
    let burn_ratio = Ratio {
        numerator: burn_ratio,
        denominator: 10,
    };
    let pool = deploy!(
        contract: StakingContractContract,
        contract_id: STAKING_POOL_ACCOUNT_ID.to_string(),
        bytes: &STAKING_FARM_BYTES,
        signer_account: root,
        // adding STAKE_SHARE_PRICE_GUARANTEE_FUND to remove this rounding issue from further calculations.
        deposit: to_yocto("10000") + 1_000_000_000_000,
        init_method: new(root.account_id(), STAKING_KEY.parse().unwrap(), reward_ratio, burn_ratio)
    );
    assert_all_success(root.call(
        token_id(),
        "storage_deposit",
        &serde_json::to_vec(&json!({ "account_id": pool.account_id() })).unwrap(),
        near_sdk_sim::DEFAULT_GAS,
        to_yocto("1"),
    ));
    call(
        &root,
        whitelist.account_id(),
        "add_staking_pool",
        json!({ "staking_pool_account_id": STAKING_POOL_ACCOUNT_ID }),
        0,
    );
    call(
        &root,
        pool.account_id(),
        "add_authorized_farm_token",
        json!({ "token_id": token_id() }),
        0,
    );
    (root, pool)
}

fn deploy_farm(root: &UserAccount) {
    let start_date = root.borrow_runtime().cur_block.block_timestamp + ONE_SEC_IN_NS * 3;
    let end_date = start_date + ONE_SEC_IN_NS * 5;
    let msg =
        serde_json::to_string(&json!({ "name": "Test", "start_date": format!("{}", start_date), "end_date": format!("{}", end_date) }))
            .unwrap();
    assert_all_success(root.call(
        token_id(),
        "ft_transfer_call",
        &serde_json::to_vec(&json!({ "receiver_id": STAKING_POOL_ACCOUNT_ID, "amount": to_yocto("50000").to_string(), "msg": msg })).unwrap(),
        near_sdk_sim::DEFAULT_GAS,
        1
    ));
}

fn assert_between(value: Balance, from: &str, to: &str) {
    assert!(
        value > to_yocto(from) && value < to_yocto(to),
        "value {} is not between {} and {}",
        value,
        to_yocto(from),
        to_yocto(to)
    );
}

fn to_int(r: ViewResult) -> Balance {
    r.unwrap_json::<U128>().0
}

fn balance_of(user: &UserAccount, account_id: AccountId) -> Balance {
    user.view(
        token_id(),
        "ft_balance_of",
        &serde_json::to_vec(&json!({ "account_id": account_id })).unwrap(),
    )
    .unwrap_json::<U128>()
    .0
}

fn create_user_and_stake(root: &UserAccount, pool: &PoolContract) -> UserAccount {
    let user1 = root.create_user(
        AccountId::new_unchecked("user1".to_string()),
        to_yocto("100000"),
    );
    storage_register(&root, user1.account_id());
    assert_all_success(call!(
        user1,
        pool.deposit_and_stake(),
        deposit = to_yocto("10000")
    ));
    user1
}

fn produce_blocks(root: &UserAccount, num_blocks: u32) {
    for _ in 0..num_blocks {
        root.borrow_runtime_mut().produce_block().unwrap();
    }
}

/// Test clean calculations without rewards and burn.
#[test]
fn test_farm() {
    let (root, pool) = setup(0, 0);
    let user1 = create_user_and_stake(&root, &pool);
    wait_epoch(&root);
    assert_all_success(call!(root, pool.ping()));
    assert_eq!(
        to_int(view!(pool.get_account_total_balance(root.account_id()))),
        0
    );
    // Half of rewards go to this user and half goes to no-one.
    assert_eq!(
        to_int(view!(pool.get_account_total_balance(user1.account_id()))),
        to_yocto("10500")
    );

    deploy_farm(&root);
    let active_farms = view!(pool.get_active_farms()).unwrap_json::<Vec<HumanReadableFarm>>();
    assert_eq!(active_farms.len(), 1);

    assert_eq!(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        to_yocto("10000"),
    );

    produce_blocks(&root, 1);

    assert_eq!(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        to_yocto("15000"),
    );

    produce_blocks(&root, 2);

    assert_eq!(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        to_yocto("25000"),
    );

    assert_all_success(call!(user1, pool.claim(token_id(), None)));
    assert_eq!(balance_of(&root, user1.account_id()), to_yocto("25000"));

    // let active_farms = view!(pool.get_active_farms()).unwrap_json::<Vec<HumanReadableFarm>>();
    // assert_eq!(active_farms.len(), 0);
}

/// Tests pool, depositing from regular account and lockup.
/// Creating two farms, farming from them, claiming via delegated call.
/// Additionally checks that 30% of rewards are burnt (sent 0x0)
#[test]
fn test_farm_with_lockup() {
    let (root, pool) = setup(1, 3);

    let user1 = create_user_and_stake(&root, &pool);
    wait_epoch(&root);
    assert_all_success(call!(root, pool.ping()));

    // Check that out of 1000 reward, 300 has burnt, 10% went to root, 50% went to "no-one", leaving ~315.
    assert_between(
        to_int(view!(pool.get_account_total_balance(root.account_id()))),
        "71",
        "72",
    );
    assert_between(
        to_int(view!(pool.get_account_total_balance(user1.account_id()))),
        "10314",
        "10315",
    );
    // Burn balance still staked.
    assert_between(
        to_int(view!(pool.get_account_total_balance(burn_account()))),
        "299",
        "300",
    );

    deploy_farm(&root);

    assert_between(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        "9900",
        "10000",
    );

    produce_blocks(&root, 1);

    assert_between(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        "14900",
        "15000",
    );

    for _ in 0..2 {
        root.borrow_runtime_mut().produce_block().unwrap();
    }

    // After 5 blocks passed, all rewards were mined.
    // The split is 10670 / 10700 went to user1, and 70 / 10700 went to root.
    // Because root received already it's reward from staking as pool owner and also participates in farming.
    assert_between(
        to_int(view!(pool.get_unclaimed_reward(user1.account_id(), 0))),
        "24914",
        "24915",
    );
    assert_between(
        to_int(view!(pool.get_unclaimed_reward(root.account_id(), 0))),
        "336",
        "337",
    );
    assert_eq!(
        to_int(view!(pool.get_unclaimed_reward(burn_account(), 0))),
        0
    );

    // Claim balance by user.
    assert_all_success(call!(user1, pool.claim(token_id(), None)));
    let claimed = balance_of(&root, user1.account_id());
    assert_between(claimed, "49639", "49640");

    assert_all_success(call!(root, pool.ping()));

    call(
        &root,
        lockup_id(),
        "select_staking_pool",
        json!({ "staking_pool_account_id": STAKING_POOL_ACCOUNT_ID }),
        0,
    );
    call(
        &root,
        lockup_id(),
        "deposit_and_stake",
        json!({ "amount": to_yocto("10000").to_string() }),
        0,
    );
    println!(
        "{:?}",
        root.borrow_runtime().view_account(STAKING_POOL_ACCOUNT_ID)
    );
    assert_all_success(call!(root, pool.ping()));

    // Deploy second farm with new period.
    deploy_farm(&root);

    // Unstake burnt tokens.
    assert_all_success(call!(root, pool.unstake_burn()));

    produce_blocks(&root, 5);

    // Note: an actual unstaking doesn't work in simulation framework,
    // which means locked balance needs to be updated manually.
    let mut account = root
        .borrow_runtime()
        .view_account(STAKING_POOL_ACCOUNT_ID)
        .unwrap();
    account.amount += to_yocto("300");
    account.locked -= to_yocto("300");
    root.borrow_runtime_mut()
        .force_account_update(pool.account_id(), &account);

    assert_between(
        to_int(view!(pool.get_unclaimed_reward(lockup_id(), 1))),
        "24148",
        "24149",
    );

    println!("before: {}", balance_of(&root, root.account_id()));
    println!(
        "unclaimed 0: {}",
        to_int(view!(pool.get_unclaimed_reward(root.account_id(), 0)))
    );
    println!(
        "unclaimed 1: {}",
        to_int(view!(pool.get_unclaimed_reward(root.account_id(), 1)))
    );

    // Claim by owner via delegated check.
    assert_all_success(call!(root, pool.claim(token_id(), Some(lockup_id()))));

    let claimed2 = balance_of(&root, root.account_id());
    assert_between(claimed2, "24148", "24149");

    // Claim from the root directly the rest.
    assert_all_success(call!(root, pool.claim(token_id(), None)));

    let claimed3 = balance_of(&root, root.account_id());
    assert_between(claimed3 - claimed2, "509", "510");

    // Actually send to burn the tokens.
    assert_all_success(call!(root, pool.burn()));

    // Burn was 30% of 1000.
    assert_between(
        root.borrow_runtime()
            .view_account(&burn_account().as_str())
            .unwrap()
            .amount,
        "299",
        "300",
    );
}
