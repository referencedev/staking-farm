use near_contract_standards::fungible_token::metadata::{FT_METADATA_SPEC, FungibleTokenMetadata};
use near_sdk::collections::{UnorderedMap, UnorderedSet, Vector};
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{
    AccountId, BorshStorageKey, EpochHeight, Gas, NearToken, PanicOnDefault, Promise,
    PromiseResult, PublicKey, env, ext_contract, near, near_bindgen,
};
use uint::construct_uint;

use crate::account::{Account, NumStakeShares};
use crate::farm::Farm;
pub use crate::views::{HumanReadableAccount, HumanReadableFarm, PoolSummary};

mod account;
mod farm;
mod internal;
mod owner;
mod stake;
#[cfg(test)]
mod test_utils;
mod token_receiver;
mod views;

/// The amount of gas given to complete internal `on_stake_action` call.
const ON_STAKE_ACTION_GAS: Gas = Gas::from_tgas(20);

/// The amount of yocto NEAR the contract dedicates to guarantee that the "share" price never
/// decreases. It's used during rounding errors for share -> amount conversions.
const STAKE_SHARE_PRICE_GUARANTEE_FUND: Balance = 1_000_000_000_000;

/// There is no deposit balance attached.
const NO_DEPOSIT: Balance = 0;

/// Maximum number of active farms at one time.
const MAX_NUM_ACTIVE_FARMS: usize = 3;

/// The number of epochs required for the locked balance to become unlocked.
/// NOTE: The actual number of epochs when the funds are unlocked is 3. But there is a corner case
/// when the unstaking promise can arrive at the next epoch, while the inner state is already
/// updated in the previous epoch. It will not unlock the funds for 4 epochs.
const NUM_EPOCHS_TO_UNLOCK: EpochHeight = 4;

/// Conservative estimate of bytes required to create a new `Account` entry in storage.
/// Used to ensure incoming FT transfers of stake shares cover storage for a new receiver.
const ACCOUNT_STORAGE_BYTES: u64 = 500;
const REGISTERED_ACCOUNT_PREFIX: &[u8] = b"regacc";

construct_uint! {
    /// 256-bit unsigned integer.
    #[near(serializers=[borsh])]
    pub struct U256(4);
}

/// Raw type for balance in yocto NEAR.
pub type Balance = u128;

#[derive(BorshStorageKey)]
#[near]
pub enum StorageKeys {
    Accounts,
    Farms,
    AuthorizedUsers,
    AuthorizedFarmTokens,
    RegisteredAccounts,
}

/// Tracking balance for burning.
#[near(serializers=[borsh])]
pub struct BurnInfo {
    /// The unstaked balance that can be burnt.
    pub unstaked: Balance,
    /// Number of "stake" shares that must be burnt.
    pub stake_shares: Balance,
    /// The minimum epoch height when the burn is allowed.
    pub unstaked_available_epoch_height: EpochHeight,
}

/// Updatable reward fee only after NUM_EPOCHS_TO_UNLOCK.
#[near(serializers=[borsh])]
pub struct UpdatableRewardFee {
    reward_fee_fraction: Ratio,
    next_reward_fee_fraction: Ratio,
    available_epoch_height: EpochHeight,
}

impl UpdatableRewardFee {
    pub fn new(reward_fee_fraction: Ratio) -> Self {
        Self {
            reward_fee_fraction: reward_fee_fraction.clone(),
            next_reward_fee_fraction: reward_fee_fraction,
            available_epoch_height: 0,
        }
    }

    pub fn current(&self) -> &Ratio {
        if env::epoch_height() >= self.available_epoch_height {
            &self.next_reward_fee_fraction
        } else {
            &self.reward_fee_fraction
        }
    }

    pub fn next(&self) -> &Ratio {
        &self.next_reward_fee_fraction
    }

    pub fn set(&mut self, next_reward_fee_fraction: Ratio) {
        if env::epoch_height() >= self.available_epoch_height {
            self.reward_fee_fraction = self.next_reward_fee_fraction.clone();
        }
        self.next_reward_fee_fraction = next_reward_fee_fraction;
        self.available_epoch_height = env::epoch_height() + NUM_EPOCHS_TO_UNLOCK
    }
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct StakingContract {
    /// The public key which is used for staking action. It's the public key of the validator node
    /// that validates on behalf of the pool.
    pub stake_public_key: PublicKey,
    /// The last epoch height when `ping` was called.
    pub last_epoch_height: EpochHeight,
    /// The last total balance of the account (consists of staked and unstaked balances).
    pub last_total_balance: Balance,
    /// The total amount of shares. It should be equal to the total amount of shares across all
    /// accounts.
    pub total_stake_shares: NumStakeShares,
    /// The total staked balance.
    pub total_staked_balance: Balance,
    /// The total burn share balance, that will not be accounted in the farming.
    pub total_burn_shares: NumStakeShares,
    /// The total amount to burn that will be available.
    /// The fraction of the reward that goes to the owner of the staking pool for running the
    /// validator node.
    pub reward_fee_fraction: UpdatableRewardFee,
    /// The fraction of the reward that gets burnt.
    pub burn_fee_fraction: Ratio,
    /// Persistent map from an account ID to the corresponding account.
    pub accounts: UnorderedMap<AccountId, Account>,
    /// Farm tokens.
    pub farms: Vector<Farm>,
    /// Active farms: indicies into `farms`.
    pub active_farms: Vec<u64>,
    /// Whether the staking is paused.
    /// When paused, the account unstakes everything (stakes 0) and doesn't restake.
    /// It doesn't affect the staking shares or reward distribution.
    /// Pausing is useful for node maintenance. Only the owner can pause and resume staking.
    /// The contract is not paused by default.
    pub paused: bool,
    /// Authorized users, allowed to add farms.
    /// This is done to prevent farm spam with random tokens.
    /// Should not be a large number.
    pub authorized_users: UnorderedSet<AccountId>,
    /// Authorized tokens for farms.
    /// Required because any contract can call method with ft_transfer_call, so must verify that contract will accept it.
    pub authorized_farm_tokens: UnorderedSet<AccountId>,
}

#[derive(Clone, PartialEq, Debug)]
#[near(serializers=[borsh, json])]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}

impl Ratio {
    pub fn assert_valid(&self) {
        assert_ne!(self.denominator, 0, "Denominator must be a positive number");
        assert!(
            self.numerator <= self.denominator,
            "The reward fee must be less or equal to 1"
        );
    }

    pub fn multiply(&self, value: Balance) -> Balance {
        if self.denominator == 0 || self.numerator == 0 {
            0
        } else {
            (U256::from(self.numerator) * U256::from(value) / U256::from(self.denominator))
                .as_u128()
        }
    }
}

#[near_bindgen]
impl StakingContract {
    /// Initializes the contract with the given owner_id, initial staking public key (with ED25519
    /// curve) and initial reward fee fraction that owner charges for the validation work.
    ///
    /// The entire current balance of this contract will be used to stake. This allows contract to
    /// always maintain staking shares that can't be unstaked or withdrawn.
    /// It prevents inflating the price of the share too much.
    #[init]
    pub fn new(
        owner_id: AccountId,
        stake_public_key: PublicKey,
        reward_fee_fraction: Ratio,
        burn_fee_fraction: Ratio,
    ) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        reward_fee_fraction.assert_valid();
        burn_fee_fraction.assert_valid();
        assert!(
            env::is_valid_account_id(owner_id.as_bytes()),
            "The owner account ID is invalid"
        );
        let account_balance = env::account_balance().as_yoctonear();
        let total_staked_balance = account_balance - STAKE_SHARE_PRICE_GUARANTEE_FUND;
        assert_eq!(
            env::account_locked_balance(),
            NearToken::from_yoctonear(0),
            "The staking pool shouldn't be staking at the initialization"
        );
        let mut this = Self {
            stake_public_key,
            last_epoch_height: env::epoch_height(),
            last_total_balance: account_balance,
            total_staked_balance,
            total_stake_shares: NumStakeShares::from(total_staked_balance),
            total_burn_shares: 0,
            reward_fee_fraction: UpdatableRewardFee::new(reward_fee_fraction),
            burn_fee_fraction,
            accounts: UnorderedMap::new(StorageKeys::Accounts),
            farms: Vector::new(StorageKeys::Farms),
            active_farms: Vec::new(),
            paused: false,
            authorized_users: UnorderedSet::new(StorageKeys::AuthorizedUsers),
            authorized_farm_tokens: UnorderedSet::new(StorageKeys::AuthorizedFarmTokens),
        };
        Self::internal_set_owner(&owner_id);
        Self::internal_set_factory(&env::predecessor_account_id());
        Self::internal_set_version();
        // Staking with the current pool to make sure the staking key is valid.
        this.internal_restake();
        this
    }

    /// Distributes rewards and restakes if needed.
    pub fn ping(&mut self) {
        if self.internal_ping() {
            self.internal_restake();
        }
    }
}

// ----------------------
// FT (NEP-141) interface
// ----------------------

/// Gas for calling external ft_on_transfer on receivers.
const GAS_FOR_FT_ON_TRANSFER: Gas = Gas::from_tgas(25);
/// Gas for resolve callback; reuse similar budget as farming resolve.
const GAS_FOR_FT_RESOLVE: Gas = Gas::from_tgas(20);

/// External interface for ft_on_transfer receivers.
#[ext_contract(ext_ft_receiver)]
pub trait ExtFtReceiver {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> near_sdk::PromiseOrValue<U128>;
}

#[near_bindgen]
impl StakingContract {
    /// Total supply equals all shares minus burned shares.
    pub fn ft_total_supply(&self) -> U128 {
        U128(
            self.total_stake_shares
                .saturating_sub(self.total_burn_shares),
        )
    }

    /// Balance of stake shares for a given account.
    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        if account_id.as_str() == crate::internal::ZERO_ADDRESS {
            return U128(0);
        }
        let account = self.internal_get_account(&account_id);
        U128(account.stake_shares)
    }

    /// Simple transfer of stake shares as FT.
    #[payable]
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128) {
        near_sdk::assert_one_yocto();
        assert!(
            receiver_id.as_str() != crate::internal::ZERO_ADDRESS,
            "ERR_TRANSFER_TO_ZERO_ADDRESS"
        );
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.0;
        assert!(amount > 0, "ERR_ZERO_AMOUNT");
        // Ensure enough shares to cover storage if receiver is a new account entry.
        self.internal_assert_receiver_storage(&receiver_id, amount);
        self.internal_share_transfer(&sender_id, &receiver_id, amount);
    }

    /// Transfer stake shares with a callback to the receiver.
    #[payable]
    pub fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        msg: String,
    ) -> Promise {
        near_sdk::assert_one_yocto();
        assert!(
            receiver_id.as_str() != crate::internal::ZERO_ADDRESS,
            "ERR_TRANSFER_TO_ZERO_ADDRESS"
        );
        let sender_id = env::predecessor_account_id();
        let amount_raw: Balance = amount.0;
        assert!(amount_raw > 0, "ERR_ZERO_AMOUNT");
        // Ensure enough shares to cover storage if receiver is a new account entry.
        self.internal_assert_receiver_storage(&receiver_id, amount_raw);

        // Perform the transfer first.
        self.internal_share_transfer(&sender_id, &receiver_id, amount_raw);

        // Initiate receiver callback and then resolve.
        ext_ft_receiver::ext(receiver_id.clone())
            // Using both static gas and unused gas weight => logic that at least static gas and add unused gas weight.
            .with_static_gas(GAS_FOR_FT_ON_TRANSFER)
            .with_unused_gas_weight(1)
            .ft_on_transfer(sender_id.clone(), amount, msg)
            .then(
                crate::stake::ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_FT_RESOLVE)
                    .ft_resolve_transfer(sender_id, receiver_id, amount),
            )
    }

    /// FT metadata (NEP-148).
    pub fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: Self::internal_get_ft_name(),
            symbol: Self::internal_get_ft_symbol(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 24,
        }
    }
}

// ------------------------------
// Storage management (NEP-145)
// ------------------------------

use near_contract_standards::storage_management::{StorageBalance, StorageBalanceBounds};

#[near_bindgen]
impl StakingContract {
    fn storage_registration_key(account_id: &AccountId) -> Vec<u8> {
        let mut key = REGISTERED_ACCOUNT_PREFIX.to_vec();
        key.extend(account_id.as_bytes());
        key
    }

    fn storage_is_registered(account_id: &AccountId) -> bool {
        env::storage_has_key(&Self::storage_registration_key(account_id))
    }

    fn storage_register_account(account_id: &AccountId) {
        env::storage_write(&Self::storage_registration_key(account_id), &[]);
    }

    fn storage_take_registration(account_id: &AccountId) -> bool {
        env::storage_remove(&Self::storage_registration_key(account_id))
    }

    fn min_storage_balance() -> NearToken {
        let byte_cost = env::storage_byte_cost().as_yoctonear();
        NearToken::from_yoctonear(byte_cost * ACCOUNT_STORAGE_BYTES as u128)
    }

    /// Returns the min and max storage balance bounds. Max is None for FTs.
    pub fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds {
            min: Self::min_storage_balance(),
            max: None,
        }
    }

    /// Returns storage balance for account if registered.
    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        if self.accounts.get(&account_id).is_none() && !Self::storage_is_registered(&account_id) {
            return None;
        }
        Some(StorageBalance {
            total: Self::min_storage_balance(),
            available: NearToken::from_yoctonear(0),
        })
    }

    /// Register an account for receiving stake shares. Excess deposit is refunded.
    #[payable]
    pub fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        _registration_only: Option<bool>,
    ) -> StorageBalance {
        let account_id = account_id.unwrap_or_else(|| env::predecessor_account_id());
        let deposit = env::attached_deposit();
        let min_balance = Self::min_storage_balance();
        let mut refund = deposit.as_yoctonear();
        if self.accounts.get(&account_id).is_none() && !Self::storage_is_registered(&account_id) {
            assert!(
                deposit.as_yoctonear() >= min_balance.as_yoctonear(),
                "ERR_INSUFFICIENT_STORAGE_DEPOSIT"
            );
            Self::storage_register_account(&account_id);
            refund -= min_balance.as_yoctonear();
        }
        if refund > 0 {
            Promise::new(env::predecessor_account_id()).transfer(NearToken::from_yoctonear(refund));
        }
        StorageBalance {
            total: min_balance,
            available: NearToken::from_yoctonear(0),
        }
    }

    /// Withdraw not supported (always zero available).
    #[payable]
    pub fn storage_withdraw(&mut self, amount: Option<U128>) -> StorageBalance {
        near_sdk::assert_one_yocto();
        let account_id = env::predecessor_account_id();
        let min = Self::min_storage_balance();
        if self.accounts.get(&account_id).is_some() {
            env::panic_str("ERR_STORAGE_IN_USE");
        }
        if !Self::storage_is_registered(&account_id) {
            return StorageBalance {
                total: NearToken::from_yoctonear(0),
                available: NearToken::from_yoctonear(0),
            };
        }
        if let Some(amount) = amount {
            assert_eq!(
                amount.0,
                min.as_yoctonear(),
                "ERR_WITHDRAW_INCORRECT_AMOUNT"
            );
        }
        Self::storage_take_registration(&account_id);
        Promise::new(account_id.clone()).transfer(min);
        StorageBalance {
            total: NearToken::from_yoctonear(0),
            available: NearToken::from_yoctonear(0),
        }
    }
}

impl StakingContract {
    /// If receiver doesn't yet have an account entry, ensure the transferred shares are enough to cover storage.
    fn internal_assert_receiver_storage(
        &mut self,
        receiver_id: &AccountId,
        _amount_shares: Balance,
    ) {
        if self.accounts.get(receiver_id).is_some() {
            return;
        }
        if Self::storage_take_registration(receiver_id) {
            return;
        }
        env::panic_str("ERR_STORAGE_NOT_REGISTERED");
    }
}

#[cfg(test)]
impl StakingContract {
    pub(crate) fn test_register_account(&mut self, account_id: &AccountId) {
        Self::storage_register_account(account_id);
    }
}

#[cfg(test)]
mod tests {
    use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
    use near_sdk::json_types::U64;
    use near_sdk::serde_json::json;
    use near_sdk::test_utils::VMContextBuilder;
    #[allow(deprecated)]
    use near_sdk::test_utils::testing_env_with_promise_results;
    use near_sdk::{NearToken, PromiseResult, testing_env};

    use crate::test_utils::tests::*;
    use crate::test_utils::*;

    use super::*;

    #[test]
    fn test_deposit_withdraw() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let deposit_amount = ntoy(1_000_000);
        emulator.update_context(bob(), deposit_amount);
        emulator.contract.deposit();
        emulator.amount += deposit_amount;
        emulator.update_context(bob(), 0);
        assert_eq!(
            emulator.contract.get_account_unstaked_balance(bob()).0,
            deposit_amount
        );
        emulator.contract.withdraw(deposit_amount.into());
        assert_eq!(
            emulator.contract.get_account_unstaked_balance(bob()).0,
            0u128
        );
    }

    #[test]
    fn test_stake_with_fee() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            Ratio {
                numerator: 10,
                denominator: 100,
            },
        );
        let deposit_amount = ntoy(1_000_000);
        emulator.update_context(bob(), deposit_amount);
        emulator.contract.deposit();
        emulator.amount += deposit_amount;
        emulator.update_context(bob(), 0);
        emulator.contract.stake(deposit_amount.into());
        emulator.simulate_stake_call();
        assert_eq!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount
        );

        let locked_amount = emulator.locked_amount;
        let n_locked_amount = yton(locked_amount);
        emulator.skip_epochs(10);
        // Overriding rewards (+ 100K reward)
        emulator.locked_amount = locked_amount + ntoy(100_000);
        emulator.update_context(bob(), 0);
        emulator.contract.ping();
        let expected_amount = deposit_amount
            + ntoy((yton(deposit_amount) * 90_000 + n_locked_amount / 2) / n_locked_amount);
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            expected_amount
        );
        // Owner got 10% of the rewards
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(owner()).0,
            ntoy(10_000)
        );

        let locked_amount = emulator.locked_amount;
        let n_locked_amount = yton(locked_amount);
        emulator.skip_epochs(10);
        // Overriding rewards (another 100K reward)
        emulator.locked_amount = locked_amount + ntoy(100_000);

        emulator.update_context(bob(), 0);
        emulator.contract.ping();
        // previous balance plus (1_090_000 / 1_100_030)% of the 90_000 reward (rounding to nearest).
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            expected_amount
                + ntoy((yton(expected_amount) * 90_000 + n_locked_amount / 2) / n_locked_amount)
        );
        // owner earns 10% with the fee and also small percentage from restaking.
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(owner()).0,
            ntoy(10_000)
                + ntoy(10_000)
                + ntoy((10_000u128 * 90_000 + n_locked_amount / 2) / n_locked_amount)
        );

        assert_eq!(emulator.contract.get_number_of_accounts(), 2);
    }

    #[test]
    fn test_stake_unstake() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let deposit_amount = ntoy(1_000_000);
        emulator.update_context(bob(), deposit_amount);
        emulator.contract.deposit();
        emulator.amount += deposit_amount;
        emulator.update_context(bob(), 0);
        emulator.contract.stake(deposit_amount.into());
        emulator.simulate_stake_call();
        assert_eq!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount
        );
        let locked_amount = emulator.locked_amount;
        // 10 epochs later, unstake half of the money.
        emulator.skip_epochs(10);
        // Overriding rewards
        emulator.locked_amount = locked_amount + ntoy(10);
        emulator.update_context(bob(), 0);
        emulator.contract.ping();
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount + ntoy(10)
        );
        emulator.contract.unstake((deposit_amount / 2).into());
        emulator.simulate_stake_call();
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount / 2 + ntoy(10)
        );
        assert_eq_in_near!(
            emulator.contract.get_account_unstaked_balance(bob()).0,
            deposit_amount / 2
        );
        let acc = emulator.contract.get_account(bob());
        assert_eq!(acc.account_id, bob());
        assert_eq_in_near!(acc.unstaked_balance.0, deposit_amount / 2);
        assert_eq_in_near!(acc.staked_balance.0, deposit_amount / 2 + ntoy(10));
        assert!(!acc.can_withdraw);

        assert!(
            !emulator
                .contract
                .is_account_unstaked_balance_available(bob()),
        );
        emulator.skip_epochs(4);
        emulator.update_context(bob(), 0);
        assert!(
            emulator
                .contract
                .is_account_unstaked_balance_available(bob()),
        );
    }

    #[test]
    fn test_stake_all_unstake_all() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let deposit_amount = ntoy(1_000_000);
        emulator.update_context(bob(), deposit_amount);
        emulator.contract.deposit_and_stake();
        emulator.amount += deposit_amount;
        emulator.simulate_stake_call();
        assert_eq!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount
        );
        assert_eq_in_near!(emulator.contract.get_account_unstaked_balance(bob()).0, 0);
        let locked_amount = emulator.locked_amount;

        // 10 epochs later, unstake all.
        emulator.skip_epochs(10);
        // Overriding rewards
        emulator.locked_amount = locked_amount + ntoy(10);
        emulator.update_context(bob(), 0);
        emulator.contract.ping();
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            deposit_amount + ntoy(10)
        );
        emulator.contract.unstake_all();
        emulator.simulate_stake_call();
        assert_eq_in_near!(emulator.contract.get_account_staked_balance(bob()).0, 0);
        assert_eq_in_near!(
            emulator.contract.get_account_unstaked_balance(bob()).0,
            deposit_amount + ntoy(10)
        );
    }

    /// Test that two can delegate and then undelegate their funds and rewards at different time.
    #[test]
    fn test_two_delegates() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.update_context(alice(), ntoy(1_000_000));
        emulator.contract.deposit();
        emulator.amount += ntoy(1_000_000);
        emulator.update_context(alice(), 0);
        emulator.contract.stake(ntoy(1_000_000).into());
        emulator.simulate_stake_call();
        emulator.skip_epochs(3);
        emulator.update_context(bob(), ntoy(1_000_000));

        emulator.contract.deposit();
        emulator.amount += ntoy(1_000_000);
        emulator.update_context(bob(), 0);
        emulator.contract.stake(ntoy(1_000_000).into());
        emulator.simulate_stake_call();
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            ntoy(1_000_000)
        );
        emulator.skip_epochs(3);
        emulator.update_context(alice(), 0);
        emulator.contract.ping();
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(alice()).0,
            ntoy(1_060_900) - 1
        );
        assert_eq_in_near!(
            emulator.contract.get_account_staked_balance(bob()).0,
            ntoy(1_030_000)
        );

        // Checking accounts view methods
        // Should be 2, because the pool has 0 fee.
        assert_eq!(emulator.contract.get_number_of_accounts(), 2);
        let accounts = emulator.contract.get_accounts(0, 10);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].account_id, alice());
        assert_eq!(accounts[1].account_id, bob());

        let accounts = emulator.contract.get_accounts(1, 10);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_id, bob());

        let accounts = emulator.contract.get_accounts(0, 1);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_id, alice());

        let accounts = emulator.contract.get_accounts(2, 10);
        assert_eq!(accounts.len(), 0);
    }

    #[test]
    fn test_low_balances() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let initial_balance = 100;
        emulator.update_context(alice(), initial_balance);
        emulator.contract.deposit();
        emulator.amount += initial_balance;
        let mut remaining = initial_balance;
        let mut amount = 1;
        while remaining >= 4 {
            emulator.update_context(alice(), 0);
            amount = 2 + (amount - 1) % 3;
            emulator.contract.stake(amount.into());
            emulator.simulate_stake_call();
            remaining -= amount;
        }
    }

    #[test]
    fn test_rewards() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let initial_balance = ntoy(100);
        emulator.update_context(alice(), initial_balance);
        emulator.contract.deposit();
        emulator.amount += initial_balance;
        let mut remaining = 100;
        let mut amount = 1;
        while remaining >= 4 {
            emulator.skip_epochs(3);
            emulator.update_context(alice(), 0);
            emulator.contract.ping();
            emulator.update_context(alice(), 0);
            amount = 2 + (amount - 1) % 3;
            emulator.contract.stake(ntoy(amount).into());
            emulator.simulate_stake_call();
            remaining -= amount;
        }
    }

    #[test]
    fn test_farm() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.update_context(owner(), 0);
        emulator.contract.add_authorized_farm_token(&bob());
        add_farm(&mut emulator, ntoy(100));

        emulator.deposit_and_stake(alice(), ntoy(1_000_000));

        let farm = emulator.contract.get_farm(0);
        assert_eq!(farm.name, "test".to_string());
        assert_eq!(farm.token_id, bob());
        assert_eq!(farm.start_date.0, 0);
        assert_eq!(farm.end_date.0, ONE_EPOCH_TS * 4);
        assert_eq!(farm.amount.0, ntoy(100));

        assert_eq!(emulator.contract.get_unclaimed_reward(alice(), 0).0, 0);

        emulator.skip_epochs(1);
        emulator.update_context(alice(), 0);

        // First user got 1/4 of the rewards after 1/4 of the time.
        assert!(almost_equal(
            emulator.contract.get_unclaimed_reward(alice(), 0).0,
            ntoy(25),
            ntoy(1) / 100
        ));

        // Adding second user.
        emulator.deposit_and_stake(charlie(), ntoy(1_000_000));

        emulator.skip_epochs(1);
        assert!(almost_equal(
            emulator.contract.get_unclaimed_reward(alice(), 0).0,
            ntoy(375612) / 10000,
            ntoy(1) / 100
        ));
        let charlie_farmed = emulator.contract.get_unclaimed_reward(charlie(), 0).0;
        assert!(almost_equal(
            charlie_farmed,
            ntoy(124388) / 10000,
            ntoy(1) / 100
        ));

        emulator.deposit_and_stake(charlie(), ntoy(1_000_000));

        // Amount is still the same after depositing more without incrementing time.
        assert_eq!(
            emulator.contract.get_unclaimed_reward(charlie(), 0).0,
            charlie_farmed
        );

        emulator.skip_epochs(1);
        assert!(almost_equal(
            emulator.contract.get_unclaimed_reward(charlie(), 0).0,
            charlie_farmed + ntoy(165834) / 10000,
            ntoy(1) / 100,
        ));

        emulator.update_context(alice(), 1);
        emulator.contract.claim(bob(), None);
        assert_eq!(emulator.contract.get_unclaimed_reward(alice(), 0).0, 0);
    }

    fn add_farm(emulator: &mut Emulator, amount: Balance) {
        emulator.update_context(bob(), 0);
        emulator.contract.ft_on_transfer(
            owner(),
            U128(amount),
            json!({
                "name": "test".to_string(),
                "start_date": U64(0),
                "end_date": U64(ONE_EPOCH_TS * 4),
            })
            .to_string(),
        );
    }

    #[test]
    fn test_stop_farm() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.update_context(owner(), 0);
        emulator.contract.add_authorized_farm_token(&bob());
        add_farm(&mut emulator, ntoy(100));
        emulator.deposit_and_stake(alice(), ntoy(1_000_000));
        emulator.skip_epochs(1);
        assert!(almost_equal(
            emulator.contract.get_unclaimed_reward(alice(), 0).0,
            ntoy(25),
            ntoy(1) / 100
        ));
        emulator.update_context(owner(), 0);
        emulator.contract.stop_farm(0);
        emulator.skip_epochs(1);
        // Deposit alice, start farm, wait for 1 epoch.
        // Stop farm, wait for another epoch - the amount of farmed tokens is the same.
        assert!(almost_equal(
            emulator.contract.get_unclaimed_reward(alice(), 0).0,
            ntoy(25),
            ntoy(1) / 100
        ));
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_AUTHORIZED_TOKEN")]
    fn test_farm_not_authorized_token() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        add_farm(&mut emulator, ntoy(100));
    }

    #[test]
    #[should_panic(expected = "ERR_FARM_AMOUNT_TOO_SMALL")]
    fn test_farm_too_small_amount() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.update_context(owner(), 0);
        emulator.contract.add_authorized_farm_token(&bob());
        add_farm(&mut emulator, 100);
    }

    #[test]
    fn test_change_reward_fee() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        assert_eq!(emulator.contract.get_reward_fee_fraction(), zero_fee());
        emulator.update_context(owner(), 0);
        let new_fee = Ratio {
            numerator: 1,
            denominator: 10,
        };
        emulator
            .contract
            .update_reward_fee_fraction(new_fee.clone());
        // The fee is still the old one.
        assert_eq!(emulator.contract.get_reward_fee_fraction(), zero_fee());
        assert_eq!(
            emulator
                .contract
                .get_pool_summary()
                .next_reward_fee_fraction,
            new_fee
        );
        emulator.skip_epochs(1);
        assert_eq!(emulator.contract.get_reward_fee_fraction(), zero_fee());
        emulator.skip_epochs(3);
        assert_eq!(emulator.contract.get_reward_fee_fraction(), new_fee);
        // Update once again.
        let new_fee2 = Ratio {
            numerator: 2,
            denominator: 10,
        };
        emulator.update_context(owner(), 0);
        emulator
            .contract
            .update_reward_fee_fraction(new_fee2.clone());
        assert_eq!(emulator.contract.get_reward_fee_fraction(), new_fee);
    }

    #[test]
    fn test_ft_metadata_defaults_and_update() {
        // By default, name = full account id; symbol = prefix before first dot of account id.
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );

        // For tests, current_account_id is set to test_utils::staking() when updating context.
        // Ensure context reflects the contract account before calling view method.
        emulator.update_context(staking(), 0);
        let md = emulator.contract.ft_metadata();
        // Defaults: name = full contract id; symbol = UPPERCASE prefix ("STAKING")
        assert_eq!(md.name, staking().to_string());
        assert_eq!(md.symbol, "STAKING".to_string());
        assert_eq!(md.decimals, 24);

        // Owner updates
        emulator.update_context(owner(), 0);
        emulator.contract.set_ft_name("My Pool Share".to_string());
        emulator.contract.set_ft_symbol("MPS".to_string());

        // Read back updated metadata
        emulator.update_context(staking(), 0);
        let md2 = emulator.contract.ft_metadata();
        assert_eq!(md2.name, "My Pool Share");
        assert_eq!(md2.symbol, "MPS");
        assert_eq!(md2.decimals, 24);
    }

    #[test]
    fn test_ft_share_transfer() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );

        // Bob stakes some NEAR to get shares.
        emulator.deposit_and_stake(bob(), ntoy(1_000_000));

        // Register Charlie for receiving shares.
        emulator.update_context(charlie(), ntoy(1));
        let _sb = emulator.contract.storage_deposit(Some(charlie()), None);

        // Check Bob's share balance and transfer half to Charlie.
        let bob_shares = emulator.contract.ft_balance_of(bob()).0;
        assert!(bob_shares > 0);
        let half = U128(bob_shares / 2);

        // Need 1 yoctoNEAR to call ft_transfer
        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(charlie(), half);

        // Balances updated: Bob decreased, Charlie increased, total preserved (excluding burn).
        let bob_after = emulator.contract.ft_balance_of(bob()).0;
        let charlie_after = emulator.contract.ft_balance_of(charlie()).0;

        assert_eq!(bob_after, bob_shares - half.0);
        assert_eq!(charlie_after, half.0);
        assert_eq!(bob_after + charlie_after, bob_shares);
    }

    #[test]
    #[should_panic(expected = "ERR_ZERO_AMOUNT")]
    fn test_ft_transfer_zero_amount_should_panic() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );

        // Bob stakes to have an account entry
        emulator.deposit_and_stake(bob(), ntoy(1));
        emulator.contract.test_register_account(&charlie());

        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(charlie(), U128(0)); // should panic ERR_ZERO_AMOUNT
    }

    #[test]
    #[should_panic(expected = "ERR_SAME_ACCOUNT")]
    fn test_ft_transfer_same_account_should_panic() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        // stake some to have shares
        emulator.deposit_and_stake(bob(), ntoy(10));

        // Try to transfer to self
        let amount = U128(ntoy(1));
        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(bob(), amount); // should panic ERR_SAME_ACCOUNT
    }

    #[test]
    #[should_panic(expected = "ERR_TRANSFER_TO_ZERO_ADDRESS")]
    fn test_ft_transfer_to_zero_address_should_panic() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.deposit_and_stake(bob(), ntoy(5));

        let zero_id: AccountId = crate::internal::ZERO_ADDRESS.parse().unwrap();
        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(zero_id, U128(ntoy(1))); // should panic ERR_TRANSFER_TO_ZERO_ADDRESS
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_SHARES")]
    fn test_ft_transfer_insufficient_shares_should_panic() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );

        // Bob stakes 1 yocto, tries to transfer more
        emulator.deposit_and_stake(bob(), ntoy(1));
        emulator.contract.test_register_account(&charlie());

        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(charlie(), U128(ntoy(2))); // should panic ERR_INSUFFICIENT_SHARES
    }

    #[test]
    #[should_panic(expected = "ERR_STORAGE_NOT_REGISTERED")]
    fn test_ft_transfer_requires_storage_registration() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.deposit_and_stake(bob(), ntoy(10));
        emulator.update_context(bob(), 1);
        emulator.contract.ft_transfer(charlie(), U128(ntoy(1)));
    }

    #[test]
    fn test_ft_resolve_transfer_refunds_unused_shares() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.deposit_and_stake(bob(), ntoy(1_000));
        emulator.contract.test_register_account(&charlie());

        let transfer_amount = ntoy(100);
        let bob_before = emulator.contract.ft_balance_of(bob()).0;

        emulator.update_context(bob(), 1);
        emulator
            .contract
            .ft_transfer(charlie(), U128(transfer_amount));

        let bob_after_transfer = emulator.contract.ft_balance_of(bob()).0;
        let charlie_after_transfer = emulator.contract.ft_balance_of(charlie()).0;
        assert_eq!(bob_after_transfer, bob_before - transfer_amount);
        assert_eq!(charlie_after_transfer, transfer_amount);

        let unused = transfer_amount / 2;
        let callback_context = VMContextBuilder::new()
            .current_account_id(staking())
            .predecessor_account_id(staking())
            .signer_account_id(staking())
            .attached_deposit(NearToken::from_yoctonear(0))
            .account_balance(NearToken::from_yoctonear(emulator.amount))
            .account_locked_balance(NearToken::from_yoctonear(emulator.locked_amount))
            .epoch_height(emulator.epoch_height)
            .block_height(emulator.block_index)
            .block_timestamp(emulator.block_timestamp)
            .build();
        #[allow(deprecated)]
        testing_env_with_promise_results(
            callback_context,
            PromiseResult::Successful(near_sdk::serde_json::to_vec(&U128(unused)).unwrap()),
        );

        let refund = emulator
            .contract
            .ft_resolve_transfer(bob(), charlie(), U128(transfer_amount));
        assert_eq!(refund.0, unused);

        let bob_after_refund = emulator.contract.ft_balance_of(bob()).0;
        let charlie_after_refund = emulator.contract.ft_balance_of(charlie()).0;
        assert_eq!(bob_after_refund, bob_after_transfer + unused);
        assert_eq!(charlie_after_refund, charlie_after_transfer - unused);
    }

    #[test]
    #[should_panic(expected = "Denominator must be a positive number")]
    fn test_new_panics_for_invalid_burn_fee_denominator() {
        let context = VMContextBuilder::new()
            .current_account_id(staking())
            .predecessor_account_id(owner())
            .signer_account_id(owner())
            .account_balance(NearToken::from_yoctonear(ntoy(1_000)))
            .build();
        testing_env!(context);
        StakingContract::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
            Ratio {
                numerator: 1,
                denominator: 0,
            },
        );
    }

    #[test]
    #[should_panic(expected = "The reward fee must be less or equal to 1")]
    fn test_new_panics_for_burn_fee_greater_than_one() {
        let context = VMContextBuilder::new()
            .current_account_id(staking())
            .predecessor_account_id(owner())
            .signer_account_id(owner())
            .account_balance(NearToken::from_yoctonear(ntoy(1_000)))
            .build();
        testing_env!(context);
        StakingContract::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
            Ratio {
                numerator: 2,
                denominator: 1,
            },
        );
    }

    #[test]
    #[should_panic(expected = "ERR_TOO_MANY_ACTIVE_FARMS")]
    fn test_active_farm_cap_enforced() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        emulator.update_context(owner(), 0);
        emulator.contract.add_authorized_farm_token(&bob());

        for i in 0..MAX_NUM_ACTIVE_FARMS {
            emulator.update_context(bob(), 0);
            emulator.contract.ft_on_transfer(
                owner(),
                U128(ntoy(100)),
                json!({
                    "name": format!("farm-{i}"),
                    "start_date": U64(0),
                    "end_date": U64(ONE_EPOCH_TS * 4),
                })
                .to_string(),
            );
        }

        emulator.update_context(bob(), 0);
        emulator.contract.ft_on_transfer(
            owner(),
            U128(ntoy(100)),
            json!({
                "name": "overflow",
                "start_date": U64(0),
                "end_date": U64(ONE_EPOCH_TS * 4),
            })
            .to_string(),
        );
    }

    #[test]
    fn test_storage_deposit_and_withdraw() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let min = emulator.contract.storage_balance_bounds().min;

        emulator.update_context(charlie(), min.as_yoctonear() + ntoy(1));
        let sb = emulator.contract.storage_deposit(Some(charlie()), None);
        assert_eq!(sb.total, min);
        assert!(emulator.contract.storage_balance_of(charlie()).is_some());

        emulator.update_context(charlie(), 1);
        let sb = emulator.contract.storage_withdraw(None);
        assert_eq!(sb.total.as_yoctonear(), 0);
        assert!(emulator.contract.storage_balance_of(charlie()).is_none());
    }

    #[test]
    #[should_panic(expected = "ERR_STORAGE_IN_USE")]
    fn test_storage_withdraw_fails_when_account_active() {
        let mut emulator = Emulator::new(
            owner(),
            "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7"
                .parse()
                .unwrap(),
            zero_fee(),
        );
        let min = emulator.contract.storage_balance_bounds().min;
        emulator.update_context(charlie(), min.as_yoctonear() + ntoy(1));
        emulator.contract.storage_deposit(Some(charlie()), None);
        emulator.deposit_and_stake(charlie(), ntoy(10));

        emulator.update_context(charlie(), 1);
        emulator.contract.storage_withdraw(None);
    }
}
