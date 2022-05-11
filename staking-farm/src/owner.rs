use near_sdk::sys;
use near_sdk::sys::{promise_batch_action_function_call, promise_batch_then};

use crate::*;

pub const OWNER_KEY: &[u8; 5] = b"OWNER";
pub const FACTORY_KEY: &[u8; 7] = b"FACTORY";
pub const VERSION_KEY: &[u8; 7] = b"VERSION";
const GET_CODE_METHOD_NAME: &[u8; 8] = b"get_code";
const GET_CODE_GAS: Gas = Gas(50_000_000_000_000);
const SELF_UPGRADE_METHOD_NAME: &[u8; 6] = b"update";
const SELF_MIGRATE_METHOD_NAME: &[u8; 7] = b"migrate";
const UPGRADE_GAS_LEFTOVER: Gas = Gas(5_000_000_000_000);
const UPDATE_GAS_LEFTOVER: Gas = Gas(5_000_000_000_000);
const NO_DEPOSIT: Balance = 0;

const ERR_MUST_BE_OWNER: &str = "Can only be called by the owner";
const ERR_MUST_BE_SELF: &str = "Can only be called by contract itself";
const ERR_MUST_BE_FACTORY: &str = "Can only be called by staking pool factory";

///*******************/
///* Owner's methods */
///*******************/
#[near_bindgen]
impl StakingContract {
    /// Storing owner in a separate storage to avoid STATE corruption issues.
    /// Returns previous owner if it existed.
    pub(crate) fn internal_set_owner(owner_id: &AccountId) -> Option<AccountId> {
        env::storage_write(OWNER_KEY, owner_id.as_bytes());
        env::storage_get_evicted()
            .map(|bytes| AccountId::new_unchecked(String::from_utf8(bytes).expect("INTERNAL FAIL")))
    }

    /// Store the factory in the storage independent of the STATE.
    pub(crate) fn internal_set_factory(factory_id: &AccountId) {
        env::storage_write(FACTORY_KEY, factory_id.as_bytes());
    }

    pub(crate) fn internal_set_version() {
        env::storage_write(VERSION_KEY, Self::internal_get_version().as_bytes());
    }

    pub(crate) fn internal_get_state_version() -> String {
        String::from_utf8(env::storage_read(VERSION_KEY).expect("MUST HAVE VERSION"))
            .expect("INTERNAL_FAIL")
    }

    /// Changes contract owner. Must be called by current owner.
    pub fn set_owner_id(owner_id: &AccountId) {
        let prev_owner = StakingContract::internal_set_owner(owner_id).expect("MUST HAVE OWNER");
        assert_eq!(
            prev_owner,
            env::predecessor_account_id(),
            "MUST BE OWNER TO SET OWNER"
        );
    }

    /// Owner's method.
    /// Updates current public key to the new given public key.
    pub fn update_staking_key(&mut self, stake_public_key: PublicKey) {
        self.assert_owner();
        // When updating the staking key, the contract has to restake.
        let _need_to_restake = self.internal_ping();
        self.stake_public_key = stake_public_key.into();
        self.internal_restake();
    }

    /// Owner's method.
    /// Updates current reward fee fraction to the new given fraction.
    pub fn update_reward_fee_fraction(&mut self, reward_fee_fraction: Ratio) {
        self.assert_owner();
        reward_fee_fraction.assert_valid();

        let need_to_restake = self.internal_ping();
        self.reward_fee_fraction.set(reward_fee_fraction);
        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Can only be called by the factory.
    /// Decreases the current burn fee fraction to the new given fraction.
    pub fn decrease_burn_fee_fraction(&mut self, burn_fee_fraction: Ratio) {
        self.assert_factory();
        burn_fee_fraction.assert_valid();
        assert!(
            u64::from(burn_fee_fraction.numerator) * u64::from(self.burn_fee_fraction.denominator)
                < u64::from(burn_fee_fraction.denominator)
                    * u64::from(self.burn_fee_fraction.numerator),
            "The factory can only decrease the burn fee fraction"
        );

        let need_to_restake = self.internal_ping();
        self.burn_fee_fraction = burn_fee_fraction;
        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Owner's method.
    /// Pauses pool staking.
    pub fn pause_staking(&mut self) {
        self.assert_owner();
        assert!(!self.paused, "The staking is already paused");

        self.internal_ping();
        self.paused = true;
        Promise::new(env::current_account_id()).stake(0, self.stake_public_key.clone());
    }

    /// Owner's method.
    /// Resumes pool staking.
    pub fn resume_staking(&mut self) {
        self.assert_owner();
        assert!(self.paused, "The staking is not paused");

        self.internal_ping();
        self.paused = false;
        self.internal_restake();
    }

    /// Add authorized user to the current contract.
    pub fn add_authorized_user(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.authorized_users.insert(&account_id);
    }

    /// Remove authorized user from the current contract.
    pub fn remove_authorized_user(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.authorized_users.remove(&account_id);
    }

    /// Add authorized token.
    pub fn add_authorized_farm_token(&mut self, token_id: &AccountId) {
        self.assert_owner_or_authorized_user();
        self.authorized_farm_tokens.insert(&token_id);
    }

    /// Remove authorized token.
    pub fn remove_authorized_farm_token(&mut self, token_id: &AccountId) {
        self.assert_owner_or_authorized_user();
        self.authorized_farm_tokens.remove(&token_id);
    }

    /// Asserts that the method was called by the owner.
    pub(crate) fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            StakingContract::internal_get_owner_id(),
            "{}",
            ERR_MUST_BE_OWNER
        );
    }

    /// Asserts that the method was called by the factory.
    pub(crate) fn assert_factory(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            StakingContract::internal_get_factory_id(),
            "{}",
            ERR_MUST_BE_FACTORY
        );
    }

    /// Asserts that the method was called by the owner or authorized user.
    pub(crate) fn assert_owner_or_authorized_user(&self) {
        let account_id = env::predecessor_account_id();
        assert!(
            account_id == StakingContract::internal_get_owner_id()
                || self.authorized_users.contains(&account_id),
            "ERR_NOT_AUTHORIZED_USER"
        );
    }
}

/// Upgrade method.
/// Takes `hash` as an argument.
/// Calls `factory_id.get_code(hash)` first to get the code.
/// Callback to `self.update(code)` to upgrade code.
/// Callback after that to `self.migrate()` to migrate the state using new code.
#[no_mangle]
pub extern "C" fn upgrade() {
    env::setup_panic_hook();
    let current_id = env::current_account_id();
    let owner_id = StakingContract::internal_get_owner_id();
    let factory_id = StakingContract::internal_get_factory_id();
    assert_eq!(
        env::predecessor_account_id(),
        owner_id,
        "{}",
        ERR_MUST_BE_OWNER
    );
    unsafe {
        // Load hash to the register 0.
        sys::input(0);
        // Create a promise for factory contract.
        let promise_id = sys::promise_batch_create(
            factory_id.as_bytes().len() as _,
            factory_id.as_bytes().as_ptr() as _,
        );
        // Call method to get the code, passing register 0 as an argument.
        promise_batch_action_function_call(
            promise_id,
            GET_CODE_METHOD_NAME.len() as _,
            GET_CODE_METHOD_NAME.as_ptr() as _,
            u64::MAX as _,
            0,
            &NO_DEPOSIT as *const u128 as _,
            GET_CODE_GAS.0,
        );
        // Add callback to actually redeploy and migrate.
        let callback_id = promise_batch_then(
            promise_id,
            current_id.as_bytes().len() as _,
            current_id.as_bytes().as_ptr() as _,
        );
        promise_batch_action_function_call(
            callback_id,
            SELF_UPGRADE_METHOD_NAME.len() as _,
            SELF_UPGRADE_METHOD_NAME.as_ptr() as _,
            0,
            0,
            &NO_DEPOSIT as *const u128 as _,
            (env::prepaid_gas() - env::used_gas() - GET_CODE_GAS - UPGRADE_GAS_LEFTOVER).0,
        );
        sys::promise_return(callback_id);
    }
}

/// Updating current contract with the received code from factory.
#[no_mangle]
pub extern "C" fn update() {
    env::setup_panic_hook();
    let current_id = env::current_account_id();
    assert_eq!(
        env::predecessor_account_id(),
        current_id,
        "{}",
        ERR_MUST_BE_SELF
    );
    unsafe {
        // Load code into register 0 result from the promise.
        match sys::promise_result(0, 0) {
            1 => {}
            // Not ready or failed.
            _ => env::panic_str("Failed to fetch the new code"),
        };
        // Update current contract with code from register 0.
        let promise_id = sys::promise_batch_create(
            current_id.as_bytes().len() as _,
            current_id.as_bytes().as_ptr() as _,
        );
        // Deploy the contract code.
        sys::promise_batch_action_deploy_contract(promise_id, u64::MAX as _, 0);
        // Call promise to migrate the state.
        // Batched together to fail upgrade if migration fails.
        promise_batch_action_function_call(
            promise_id,
            SELF_MIGRATE_METHOD_NAME.len() as _,
            SELF_MIGRATE_METHOD_NAME.as_ptr() as _,
            0,
            0,
            &NO_DEPOSIT as *const u128 as _,
            (env::prepaid_gas() - env::used_gas() - UPDATE_GAS_LEFTOVER).0,
        );
        sys::promise_return(promise_id);
    }
}

/// Empty migrate method for future use.
/// Makes sure that state version is previous.
/// When updating code, make sure to update what previous version actually is.
#[no_mangle]
pub extern "C" fn migrate() {
    env::setup_panic_hook();
    assert_eq!(
        env::predecessor_account_id(),
        env::current_account_id(),
        "{}",
        ERR_MUST_BE_SELF
    );

    // Check that state version is previous.
    // Will fail migration in the case of trying to skip the versions.
    assert_eq!(
        StakingContract::internal_get_state_version(),
        "staking-farm:1.0.0"
    );
    StakingContract::internal_set_version();
}
