use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedSet;
use near_sdk::json_types::{Base58CryptoHash, U128};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::serde_json::json;
use near_sdk::{
    assert_self, env, ext_contract, is_promise_success, log, near_bindgen, sys, AccountId, Balance,
    CryptoHash, PanicOnDefault, Promise, PromiseOrValue, PublicKey,
};

/// The 30 NEAR tokens required for the storage of the staking pool.
const MIN_ATTACHED_BALANCE: Balance = 30_000_000_000_000_000_000_000_000;

const NEW_METHOD_NAME: &str = "new";
const ON_STAKING_POOL_CREATE: &str = "on_staking_pool_create";

/// There is no deposit balance attached.
const NO_DEPOSIT: Balance = 0;

pub mod gas {
    use near_sdk::Gas;

    /// The base amount of gas for a regular execution.
    const BASE: Gas = Gas(25_000_000_000_000);

    /// The amount of Gas the contract will attach to the promise to create the staking pool.
    /// The base for the execution and the base for staking action to verify the staking key.
    pub const STAKING_POOL_NEW: Gas = Gas(BASE.0 * 2);

    /// The amount of Gas the contract will attach to the callback to itself.
    /// The base for the execution and the base for whitelist call or cash rollback.
    pub const CALLBACK: Gas = Gas(BASE.0 * 2);

    /// The amount of Gas the contract will attach to the promise to the whitelist contract.
    /// The base for the execution.
    pub const WHITELIST_STAKING_POOL: Gas = BASE;
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct StakingPoolFactory {
    /// Account ID that can upload new staking contracts.
    owner_id: AccountId,

    /// Account ID of the staking pool whitelist contract.
    staking_pool_whitelist_account_id: AccountId,

    /// The account ID of the staking pools created.
    staking_pool_account_ids: UnorderedSet<AccountId>,
}

/// Rewards fee fraction structure for the staking pool contract.
#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct RewardFeeFraction {
    pub numerator: u32,
    pub denominator: u32,
}

impl RewardFeeFraction {
    pub fn assert_valid(&self) {
        assert_ne!(self.denominator, 0, "Denominator must be a positive number");
        assert!(
            self.numerator <= self.denominator,
            "The reward fee must be less or equal to 1"
        );
    }
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct StakingPoolArgs {
    /// Owner account ID of the staking pool.
    owner_id: AccountId,
    /// The initial staking key.
    stake_public_key: PublicKey,
    /// The initial reward fee fraction.
    reward_fee_fraction: RewardFeeFraction,
}

/// External interface for the callbacks to self.
#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_staking_pool_create(
        &mut self,
        staking_pool_account_id: AccountId,
        attached_deposit: U128,
        predecessor_account_id: AccountId,
    ) -> Promise;
}

/// External interface for the whitelist contract.
#[ext_contract(ext_whitelist)]
pub trait ExtWhitelist {
    fn add_staking_pool(&mut self, staking_pool_account_id: AccountId) -> bool;
}

#[near_bindgen]
impl StakingPoolFactory {
    /// Initializes the staking pool factory with the given account ID of the staking pool whitelist
    /// contract.
    #[init]
    pub fn new(owner_id: AccountId, staking_pool_whitelist_account_id: AccountId) -> Self {
        assert!(
            env::is_valid_account_id(owner_id.as_bytes()),
            "The owner account ID is invalid"
        );
        assert!(
            env::is_valid_account_id(staking_pool_whitelist_account_id.as_bytes()),
            "The staking pool whitelist account ID is invalid"
        );
        Self {
            owner_id,
            staking_pool_whitelist_account_id,
            staking_pool_account_ids: UnorderedSet::new(b"s".to_vec()),
        }
    }

    /// Returns the minimum amount of tokens required to attach to the function call to
    /// create a new staking pool.
    pub fn get_min_attached_balance(&self) -> U128 {
        MIN_ATTACHED_BALANCE.into()
    }

    /// Returns the total number of the staking pools created from this factory.
    pub fn get_number_of_staking_pools_created(&self) -> u64 {
        self.staking_pool_account_ids.len()
    }

    /// Creates a new staking pool.
    /// - `staking_pool_id` - the prefix of the account ID that will be used to create a new staking
    ///    pool account. It'll be prepended to the staking pool factory account ID separated by dot.
    /// - `code_hash` - hash of the code that should be deployed.
    /// - `owner_id` - the account ID of the staking pool owner. This account will be able to
    ///    control the staking pool, set reward fee, update staking key and vote on behalf of the
    ///     pool.
    /// - `stake_public_key` - the initial staking key for the staking pool.
    /// - `reward_fee_fraction` - the initial reward fee fraction for the staking pool.
    #[payable]
    pub fn create_staking_pool(
        &mut self,
        staking_pool_id: String,
        code_hash: Base58CryptoHash,
        owner_id: AccountId,
        stake_public_key: PublicKey,
        reward_fee_fraction: RewardFeeFraction,
    ) {
        assert!(
            env::attached_deposit() >= MIN_ATTACHED_BALANCE,
            "Not enough attached deposit to complete staking pool creation"
        );

        assert!(
            staking_pool_id.find('.').is_none(),
            "The staking pool ID can't contain `.`"
        );

        let staking_pool_account_id: AccountId =
            format!("{}.{}", staking_pool_id, env::current_account_id())
                .parse()
                .unwrap();
        assert!(
            env::is_valid_account_id(staking_pool_account_id.as_bytes()),
            "The staking pool account ID is invalid"
        );

        assert!(
            env::is_valid_account_id(owner_id.as_bytes()),
            "The owner account ID is invalid"
        );
        reward_fee_fraction.assert_valid();

        assert!(
            self.staking_pool_account_ids
                .insert(&staking_pool_account_id),
            "The staking pool account ID already exists"
        );

        create_contract(
            staking_pool_account_id,
            code_hash.into(),
            StakingPoolArgs {
                owner_id,
                stake_public_key,
                reward_fee_fraction,
            },
        );
    }

    /// Callback after a staking pool was created.
    /// Returns the promise to whitelist the staking pool contract if the pool creation succeeded.
    /// Otherwise refunds the attached deposit and returns `false`.
    pub fn on_staking_pool_create(
        &mut self,
        staking_pool_account_id: AccountId,
        attached_deposit: U128,
        predecessor_account_id: AccountId,
    ) -> PromiseOrValue<bool> {
        assert_self();

        let staking_pool_created = is_promise_success();

        if staking_pool_created {
            log!(
                "The staking pool @{} was successfully created. Whitelisting...",
                staking_pool_account_id
            );
            ext_whitelist::add_staking_pool(
                staking_pool_account_id,
                self.staking_pool_whitelist_account_id.clone(),
                NO_DEPOSIT,
                gas::WHITELIST_STAKING_POOL,
            )
            .into()
        } else {
            self.staking_pool_account_ids
                .remove(&staking_pool_account_id);
            log!(
                "The staking pool @{} creation has failed. Returning attached deposit of {} to @{}",
                staking_pool_account_id,
                attached_deposit.0,
                predecessor_account_id
            );
            Promise::new(predecessor_account_id).transfer(attached_deposit.0);
            PromiseOrValue::Value(false)
        }
    }

    /// Returns code at the given hash.
    pub fn get_code(&self, code_hash: Base58CryptoHash) {
        let code_hash: CryptoHash = code_hash.into();
        unsafe {
            // Check that such contract exists.
            assert_eq!(
                sys::storage_has_key(code_hash.len() as _, code_hash.as_ptr() as _),
                1,
                "Contract doesn't exist"
            );
            // Load the hash from storage.
            sys::storage_read(code_hash.len() as _, code_hash.as_ptr() as _, 0);
            // Return as value.
            sys::value_return(u64::MAX as _, 0 as _);
        }
    }
}

fn store_contract() {
    unsafe {
        // Load input into register 0.
        sys::input(0);
        // Compute sha256 hash of register 0 and store in 1.
        sys::sha256(u64::MAX as _, 0 as _, 1);
        // Check if such blob already stored.
        assert_eq!(
            sys::storage_has_key(u64::MAX as _, 1 as _),
            0,
            "ERR_ALREADY_EXISTS"
        );
        // Store value of register 0 into key = register 1.
        sys::storage_write(u64::MAX as _, 1 as _, u64::MAX as _, 0 as _, 2);
        // Load register 1 into blob_hash.
        let blob_hash = [0u8; 32];
        sys::read_register(1, blob_hash.as_ptr() as _);
        // Return from function value of register 1.
        let blob_hash_str = near_sdk::serde_json::to_string(&Base58CryptoHash::from(blob_hash))
            .unwrap()
            .into_bytes();
        sys::value_return(blob_hash_str.len() as _, blob_hash_str.as_ptr() as _);
    }
}

fn create_contract(
    staking_pool_account_id: AccountId,
    code_hash: CryptoHash,
    args: StakingPoolArgs,
) {
    let attached_deposit = env::attached_deposit();
    let factory_account_id = env::current_account_id().as_bytes().to_vec();
    let encoded_args = near_sdk::serde_json::to_vec(&args).expect("Failed to serialize");
    let callback_args = near_sdk::serde_json::to_vec(&json!({
        "staking_pool_account_id": staking_pool_account_id,
        "attached_deposit": format!("{}", attached_deposit),
        "predecessor_account_id": env::predecessor_account_id(),
    }))
    .expect("Failed to serialize");
    let staking_pool_account_id = staking_pool_account_id.as_bytes().to_vec();
    unsafe {
        // Check that such contract exists.
        assert_eq!(
            sys::storage_has_key(code_hash.len() as _, code_hash.as_ptr() as _),
            1,
            "Contract doesn't exist"
        );
        // Load input (wasm code) into register 0.
        sys::storage_read(code_hash.len() as _, code_hash.as_ptr() as _, 0);
        // schedule a Promise tx to account_id
        let promise_id = sys::promise_batch_create(
            staking_pool_account_id.len() as _,
            staking_pool_account_id.as_ptr() as _,
        );
        // create account first.
        sys::promise_batch_action_create_account(promise_id);
        // transfer attached deposit.
        sys::promise_batch_action_transfer(promise_id, &attached_deposit as *const u128 as _);
        // deploy contract (code is taken from register 0).
        sys::promise_batch_action_deploy_contract(promise_id, u64::MAX as _, 0);
        // call `new` with given arguments.
        sys::promise_batch_action_function_call(
            promise_id,
            NEW_METHOD_NAME.len() as _,
            NEW_METHOD_NAME.as_ptr() as _,
            encoded_args.len() as _,
            encoded_args.as_ptr() as _,
            &NO_DEPOSIT as *const u128 as _,
            gas::STAKING_POOL_NEW.0,
        );
        // attach callback to the factory.
        let _ = sys::promise_then(
            promise_id,
            factory_account_id.len() as _,
            factory_account_id.as_ptr() as _,
            ON_STAKING_POOL_CREATE.len() as _,
            ON_STAKING_POOL_CREATE.as_ptr() as _,
            callback_args.len() as _,
            callback_args.as_ptr() as _,
            &NO_DEPOSIT as *const u128 as _,
            gas::CALLBACK.0,
        );
        sys::promise_return(promise_id);
    }
}

/// Store new staking contract. Only owner.
#[no_mangle]
pub extern "C" fn store() {
    env::setup_panic_hook();
    let contract: StakingPoolFactory = env::state_read().expect("Contract is not initialized");
    assert_eq!(
        contract.owner_id,
        env::predecessor_account_id(),
        "Must be owner"
    );
    store_contract();
}

#[cfg(test)]
mod tests {
    use near_sdk::env::sha256;
    use near_sdk::test_utils::{testing_env_with_promise_results, VMContextBuilder};
    use near_sdk::{testing_env, PromiseResult, VMContext};

    use super::*;

    pub fn account_near() -> AccountId {
        "near".parse().unwrap()
    }
    pub fn account_whitelist() -> AccountId {
        "whitelist".parse().unwrap()
    }
    pub fn staking_pool_id() -> String {
        "pool".to_string()
    }
    pub fn account_pool() -> AccountId {
        "pool.factory".parse().unwrap()
    }
    pub fn account_factory() -> AccountId {
        "factory".parse().unwrap()
    }
    pub fn account_tokens_owner() -> AccountId {
        "tokens-owner".parse().unwrap()
    }
    pub fn account_pool_owner() -> AccountId {
        "pool-owner".parse().unwrap()
    }

    pub fn ntoy(near_amount: Balance) -> Balance {
        near_amount * 10u128.pow(24)
    }

    pub fn get_hash(data: &[u8]) -> Base58CryptoHash {
        let hash = sha256(&data);
        let mut result: CryptoHash = [0; 32];
        result.copy_from_slice(&hash);
        Base58CryptoHash::from(result)
    }

    pub fn add_staking_contract(context: &mut VMContext) -> Base58CryptoHash {
        context.input = include_bytes!("../../res/staking_farm_local.wasm").to_vec();
        let hash = get_hash(&context.input);
        testing_env!(context.clone());
        store_contract();
        context.input = vec![];
        hash
    }

    #[test]
    fn test_create_staking_pool_success() {
        let mut context = VMContextBuilder::new()
            .current_account_id(account_factory())
            .predecessor_account_id(account_near())
            .build();
        testing_env!(context.clone());

        let mut contract = StakingPoolFactory::new(account_near(), account_whitelist());
        let hash = add_staking_contract(&mut context);

        context.input = vec![];
        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_min_attached_balance().0, MIN_ATTACHED_BALANCE);
        assert_eq!(contract.get_number_of_staking_pools_created(), 0);

        context.is_view = false;
        context.predecessor_account_id = account_tokens_owner().into();
        context.attached_deposit = ntoy(31);
        testing_env!(context.clone());
        contract.create_staking_pool(
            staking_pool_id(),
            hash,
            account_pool_owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            RewardFeeFraction {
                numerator: 10,
                denominator: 100,
            },
        );

        context.predecessor_account_id = account_factory().into();
        context.attached_deposit = ntoy(0);
        testing_env_with_promise_results(context.clone(), PromiseResult::Successful(vec![]));
        contract.on_staking_pool_create(account_pool(), ntoy(31).into(), account_tokens_owner());

        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_number_of_staking_pools_created(), 1);
    }

    #[test]
    #[should_panic(expected = "Not enough attached deposit to complete staking pool creation")]
    fn test_create_staking_pool_not_enough_deposit() {
        let mut context = VMContextBuilder::new()
            .current_account_id(account_factory())
            .predecessor_account_id(account_near())
            .build();
        testing_env!(context.clone());

        let mut contract = StakingPoolFactory::new(account_near(), account_whitelist());
        let hash = add_staking_contract(&mut context);

        // Checking the pool is still whitelisted
        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_min_attached_balance().0, MIN_ATTACHED_BALANCE);
        assert_eq!(contract.get_number_of_staking_pools_created(), 0);

        context.is_view = false;
        context.predecessor_account_id = account_tokens_owner().into();
        context.attached_deposit = ntoy(20);
        testing_env!(context.clone());
        contract.create_staking_pool(
            staking_pool_id(),
            hash,
            account_pool_owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            RewardFeeFraction {
                numerator: 10,
                denominator: 100,
            },
        );
    }

    #[test]
    fn test_create_staking_pool_rollback() {
        let mut context = VMContextBuilder::new()
            .current_account_id(account_factory())
            .predecessor_account_id(account_near())
            .build();
        testing_env!(context.clone());

        let mut contract = StakingPoolFactory::new(account_near(), account_whitelist());
        let hash = add_staking_contract(&mut context);

        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_min_attached_balance().0, MIN_ATTACHED_BALANCE);
        assert_eq!(contract.get_number_of_staking_pools_created(), 0);

        context.is_view = false;
        context.predecessor_account_id = account_tokens_owner().into();
        context.attached_deposit = ntoy(31);
        testing_env!(context.clone());
        contract.create_staking_pool(
            staking_pool_id(),
            hash,
            account_pool_owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            RewardFeeFraction {
                numerator: 10,
                denominator: 100,
            },
        );

        context.predecessor_account_id = account_factory().into();
        context.attached_deposit = ntoy(0);
        context.account_balance += ntoy(31);
        testing_env_with_promise_results(context.clone(), PromiseResult::Failed);
        let res = contract.on_staking_pool_create(
            account_pool(),
            ntoy(31).into(),
            account_tokens_owner(),
        );
        match res {
            PromiseOrValue::Promise(_) => panic!("Unexpected result, should return Value(false)"),
            PromiseOrValue::Value(value) => assert!(!value),
        };

        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_number_of_staking_pools_created(), 0);
    }
}
