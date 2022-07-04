use crate::owner::{FACTORY_KEY, OWNER_KEY};
use crate::stake::ext_self;
use crate::*;
use near_sdk::log;
use crate::staking_pool::{StakingPool};
/// Zero address is implicit address that doesn't have a key for it.
/// Used for burning tokens.
pub const ZERO_ADDRESS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Minimum amount that will be sent to burn. This is to ensure there is enough storage on the other side.
pub const MIN_BURN_AMOUNT: Balance = 1694457700619870000000;

impl StakingContract {
    /********************/
    /* Internal methods */
    /********************/

    /// Restakes the current `total_staked_balance` again.
    pub(crate) fn internal_restake(&mut self) {
        if self.paused {
            return;
        }
        // Stakes with the staking public key. If the public key is invalid the entire function
        // call will be rolled back.
        Promise::new(env::current_account_id())
            .stake(self.get_total_staked_balance().0, self.stake_public_key.clone())
            .then(ext_self::on_stake_action(
                env::current_account_id(),
                NO_DEPOSIT,
                ON_STAKE_ACTION_GAS,
            ));
    }

    pub(crate) fn internal_deposit(&mut self, should_staking_pool_restake_rewards: bool) -> u128 {
        let account_id = env::predecessor_account_id();
        let staking_pool = self.get_staking_pool_or_create(&account_id, should_staking_pool_restake_rewards);
        let amount = env::attached_deposit();
        
        staking_pool.deposit(&account_id, amount);
        self.last_total_balance += amount;

        amount
    }

    pub(crate) fn internal_withdraw(&mut self, account_id: &AccountId, receiver_account_id: AccountId, amount: Balance, withdraw_rewards: bool) {
        assert!(amount > 0, "Withdrawal amount should be positive");

        let staking_pool = self.get_staking_pool_or_assert_if_not_present(&account_id);
        let mut total_withdraw = 0u128;
        if withdraw_rewards {
            let (rewards, _)= staking_pool.withdraw_not_staked_rewards(&account_id);
            total_withdraw += rewards;
        }
        let should_remove_account_from_staking_pool_register = staking_pool.withdraw(&account_id, amount);
        if should_remove_account_from_staking_pool_register{
            self.account_pool_register.remove(&account_id);
        }        
        total_withdraw += amount;
        Promise::new(receiver_account_id.clone()).transfer(total_withdraw);
        self.last_total_balance -= total_withdraw;
    }

    pub(crate) fn internal_stake(&mut self, amount: Balance) {
        assert!(amount > 0, "Staking amount should be positive");

        let account_id = env::predecessor_account_id();
        let account_staking_rewards = self.does_account_stake_his_rewards(&account_id);

        if account_staking_rewards {
            let mut account = self.rewards_staked_staking_pool.get_account_impl(&account_id);
            self.internal_distribute_all_rewards(account.as_mut(), true);
            self.rewards_staked_staking_pool.stake(&account_id, amount, account.as_mut());
        } else {
            let mut account = self.rewards_not_staked_staking_pool.get_account_impl(&account_id);
            self.internal_distribute_all_rewards(account.as_mut(), false);
            self.rewards_not_staked_staking_pool.stake(&account_id, amount, account.as_mut());
        }
    }

    pub(crate) fn inner_unstake(&mut self, account_id: &AccountId, amount: u128) {
        assert!(amount > 0, "Unstaking amount should be positive");

        let account_staking_rewards = self.does_account_stake_his_rewards(&account_id);
        if account_staking_rewards {
            let mut account = self.rewards_staked_staking_pool.get_account_impl(&account_id);
            self.internal_distribute_all_rewards(account.as_mut(), true);
            self.rewards_staked_staking_pool.unstake(&account_id, amount, account.as_mut());
        } else {
            let mut account = self.rewards_not_staked_staking_pool.get_account_impl(&account_id);
            self.internal_distribute_all_rewards(account.as_mut(), false);
            self.rewards_not_staked_staking_pool.unstake(&account_id, amount, account.as_mut());
        }
    }

    pub(crate) fn internal_unstake_all(&mut self, account_id: &AccountId) {
        // Unstake action always restakes
        self.internal_ping();

        let staked_balance = self.get_account_staked_balance(account_id.clone());
        self.inner_unstake(account_id, staked_balance.0);

        self.internal_restake();
    }

    /// Add given number of staked shares to the given account.
    fn internal_add_shares(&mut self, account_id: &AccountId, num_shares: NumStakeShares) {
        if num_shares > 0 {
            let mut account = self.rewards_staked_staking_pool.internal_get_account(&account_id);
            account.stake_shares += num_shares;
            self.rewards_staked_staking_pool.internal_save_account(&account_id, &account);
            // Increasing the total amount of "stake" shares.
            self.rewards_staked_staking_pool.total_stake_shares += num_shares;
        }
    }

    /// Distributes rewards after the new epoch. It's automatically called before every action.
    /// Returns true if the current epoch height is different from the last epoch height.
    pub(crate) fn internal_ping(&mut self) -> bool {
        let epoch_height = env::epoch_height();
        if self.last_epoch_height == epoch_height {
            return false;
        }
        self.last_epoch_height = epoch_height;

        // New total amount (both locked and unlocked balances).
        // NOTE: We need to subtract `attached_deposit` in case `ping` called from `deposit` call
        // since the attached deposit gets included in the `account_balance`, and we have not
        // accounted it yet.
        let total_balance =
            env::account_locked_balance() + env::account_balance() - env::attached_deposit();

        assert!(
            total_balance >= self.last_total_balance,
            "The new total balance should not be less than the old total balance {} {}",
            total_balance,
            self.last_total_balance
        );
        let total_reward = total_balance - self.last_total_balance;
        if total_reward > 0 {
            // The validation fee that will be burnt.
            let burn_fee = self.burn_fee_fraction.multiply(total_reward);

            // The validation fee that the contract owner takes.
            let owners_fee = self
                .reward_fee_fraction
                .current()
                .multiply(total_reward - burn_fee);

            // Distributing the remaining reward to the delegators first.
            let remaining_reward = total_reward - owners_fee - burn_fee;

            // Distribute remaining reward between the two pools
            let not_staked_rewards = self.part_of_amount_based_on_stake_share_rounded_down(
                remaining_reward, 
                self.rewards_not_staked_staking_pool.total_staked_balance);

            self.rewards_not_staked_staking_pool.distribute_reward(not_staked_rewards);
            self.rewards_staked_staking_pool.total_staked_balance += remaining_reward - not_staked_rewards;

            // Now buying "stake" shares for the burn.
            let num_burn_shares = self.rewards_staked_staking_pool.num_shares_from_staked_amount_rounded_down(burn_fee);
            self.rewards_staked_staking_pool.total_burn_shares += num_burn_shares;

            // Now buying "stake" shares for the contract owner at the new share price.
            let num_owner_shares = self.rewards_staked_staking_pool.num_shares_from_staked_amount_rounded_down(owners_fee);

            self.internal_add_shares(
                &AccountId::new_unchecked(ZERO_ADDRESS.to_string()),
                num_burn_shares,
            );
            self.internal_add_shares(&StakingContract::internal_get_owner_id(), num_owner_shares);

            // Increasing the total staked balance by the owners fee, no matter whether the owner
            // received any shares or not.
            self.rewards_staked_staking_pool.total_staked_balance += owners_fee + burn_fee;

            log!(
                "Epoch {}: Contract received total rewards of {} tokens. \
                 New total staked balance is {}. Total number of shares {}",
                epoch_height,
                total_reward,
                self.rewards_staked_staking_pool.total_staked_balance,
                self.rewards_staked_staking_pool.total_stake_shares,
            );
            if num_owner_shares > 0 || num_burn_shares > 0 {
                log!(
                    "Total rewards fee is {} and burn is {} stake shares.",
                    num_owner_shares,
                    num_burn_shares
                );
            }
        }

        self.last_total_balance = total_balance;
        true
    }
    
    /// Register account to which staking pool it belongs
    pub(crate) fn internal_register_account_to_staking_pool(&mut self, account_id: &AccountId, do_stake_rewards: bool){
        self.account_pool_register.insert(account_id, &do_stake_rewards);
    }

    /// Get staking pool for account id, if its not present create it
    /// based on client intention, if he wants his rewards to be restaked or not
    pub(crate) fn get_staking_pool_or_create(&mut self, account_id: &AccountId, should_staking_pool_restake_rewards: bool) -> &mut dyn StakingPool{
        let account_staking_pool_option = self.account_pool_register.get(&account_id);

        if account_staking_pool_option.is_none(){
            self.internal_register_account_to_staking_pool(account_id, should_staking_pool_restake_rewards);
        }

        if account_staking_pool_option.unwrap_or(should_staking_pool_restake_rewards) {
            return &mut self.rewards_staked_staking_pool;
        }else{
            return &mut self.rewards_not_staked_staking_pool;
        }
    }

    /// Get inner staking pool associated with account or default inner staking pool
    pub(crate) fn get_staking_pool_or_default(&self, account_id: &AccountId) -> &dyn StakingPool{
        let account_staking_pool_option = self.account_pool_register.get(&account_id);

        if account_staking_pool_option.unwrap_or(true){
            return &self.rewards_staked_staking_pool;
        }else{
            return &self.rewards_not_staked_staking_pool;
        }
    }

    fn get_staking_pool_or_assert_if_not_present(&mut self, account_id: &AccountId) -> &mut dyn StakingPool{
        let account_staking_pool_option = self.account_pool_register.get(&account_id);
        assert!(account_staking_pool_option.is_some(), "Account {} should be registered for one of the staking pools", account_id);

        if account_staking_pool_option.unwrap() {
            return &mut self.rewards_staked_staking_pool;
        }else{
            return &mut self.rewards_not_staked_staking_pool;
        }
    }

    pub fn does_account_stake_his_rewards(&self, account_id: &AccountId) -> bool{
        let account_staking_pool_option = self.account_pool_register.get(&account_id);
        assert!(account_staking_pool_option.is_some(), "Account {} should be registered for one of the staking pools", account_id);

        return account_staking_pool_option.unwrap();
    }

    fn part_of_amount_based_on_stake_share_rounded_down(&self, amount: Balance, stake:Balance) -> Balance{
        (U256::from(stake) * U256::from(amount)
            / (U256::from(self.rewards_staked_staking_pool.total_staked_balance) 
            + U256::from(self.rewards_not_staked_staking_pool.total_staked_balance)))
        .as_u128()
    }

    pub(crate) fn internal_withdraw_rewards(&mut self, receiver_account_id: &AccountId){
        assert!(
            env::is_valid_account_id(receiver_account_id.as_bytes()),
            "The receiver account ID is invalid"
        );
        let account_id = env::predecessor_account_id();
        let staking_pool = self.get_staking_pool_or_assert_if_not_present(&account_id);
        let (reward, account_should_be_removed) = staking_pool.withdraw_not_staked_rewards(&account_id);
        if account_should_be_removed {
            self.account_pool_register.remove(&account_id);
        }

        if reward != 0 {
            Promise::new(receiver_account_id.clone()).transfer(reward);
            self.last_total_balance -= reward;
        }
    }

    /// Returns current contract version.
    pub(crate) fn internal_get_version() -> String {
        format!("{}:{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    }

    /// Returns current owner from the storage.
    pub(crate) fn internal_get_owner_id() -> AccountId {
        AccountId::new_unchecked(
            String::from_utf8(env::storage_read(OWNER_KEY).expect("MUST HAVE OWNER"))
                .expect("INTERNAL_FAIL"),
        )
    }

    /// Returns current contract factory.
    pub(crate) fn internal_get_factory_id() -> AccountId {
        AccountId::new_unchecked(
            String::from_utf8(env::storage_read(FACTORY_KEY).expect("MUST HAVE FACTORY"))
                .expect("INTERNAL_FAIL"),
        )
    }
}
