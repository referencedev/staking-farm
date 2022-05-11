use crate::owner::{FACTORY_KEY, OWNER_KEY};
use crate::stake::ext_self;
use crate::*;
use near_sdk::log;

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
            .stake(self.total_staked_balance, self.stake_public_key.clone())
            .then(ext_self::on_stake_action(
                env::current_account_id(),
                NO_DEPOSIT,
                ON_STAKE_ACTION_GAS,
            ));
    }

    pub(crate) fn internal_deposit(&mut self) -> u128 {
        let account_id = env::predecessor_account_id();
        let mut account = self.internal_get_account(&account_id);
        let amount = env::attached_deposit();
        account.unstaked += amount;
        self.internal_save_account(&account_id, &account);
        self.last_total_balance += amount;

        log!(
            "@{} deposited {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );
        amount
    }

    pub(crate) fn internal_withdraw(&mut self, account_id: &AccountId, amount: Balance) {
        assert!(amount > 0, "Withdrawal amount should be positive");

        let mut account = self.internal_get_account(&account_id);
        assert!(
            account.unstaked >= amount,
            "Not enough unstaked balance to withdraw"
        );
        assert!(
            account.unstaked_available_epoch_height <= env::epoch_height(),
            "The unstaked balance is not yet available due to unstaking delay"
        );
        account.unstaked -= amount;
        self.internal_save_account(&account_id, &account);

        log!(
            "@{} withdrawing {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );

        Promise::new(account_id.clone()).transfer(amount);
        self.last_total_balance -= amount;
    }

    pub(crate) fn internal_stake(&mut self, amount: Balance) {
        assert!(amount > 0, "Staking amount should be positive");

        let account_id = env::predecessor_account_id();
        let mut account = self.internal_get_account(&account_id);

        // Distribute rewards from all the farms for the given user.
        self.internal_distribute_all_rewards(&mut account);

        // Calculate the number of "stake" shares that the account will receive for staking the
        // given amount.
        let num_shares = self.num_shares_from_staked_amount_rounded_down(amount);
        assert!(
            num_shares > 0,
            "The calculated number of \"stake\" shares received for staking should be positive"
        );
        // The amount of tokens the account will be charged from the unstaked balance.
        // Rounded down to avoid overcharging the account to guarantee that the account can always
        // unstake at least the same amount as staked.
        let charge_amount = self.staked_amount_from_num_shares_rounded_down(num_shares);
        assert!(
            charge_amount > 0,
            "Invariant violation. Calculated staked amount must be positive, because \"stake\" share price should be at least 1"
        );

        assert!(
            account.unstaked >= charge_amount,
            "Not enough unstaked balance to stake"
        );
        account.unstaked -= charge_amount;
        account.stake_shares += num_shares;
        self.internal_save_account(&account_id, &account);

        // The staked amount that will be added to the total to guarantee the "stake" share price
        // never decreases. The difference between `stake_amount` and `charge_amount` is paid
        // from the allocated STAKE_SHARE_PRICE_GUARANTEE_FUND.
        let stake_amount = self.staked_amount_from_num_shares_rounded_up(num_shares);

        self.total_staked_balance += stake_amount;
        self.total_stake_shares += num_shares;

        log!(
            "@{} staking {}. Received {} new staking shares. Total {} unstaked balance and {} \
             staking shares",
            account_id,
            charge_amount,
            num_shares,
            account.unstaked,
            account.stake_shares
        );
        log!(
            "Contract total staked balance is {}. Total number of shares {}",
            self.total_staked_balance,
            self.total_stake_shares
        );
    }

    pub(crate) fn inner_unstake(&mut self, account_id: &AccountId, amount: u128) {
        assert!(amount > 0, "Unstaking amount should be positive");

        let mut account = self.internal_get_account(&account_id);

        // Distribute rewards from all the farms for the given user.
        self.internal_distribute_all_rewards(&mut account);

        assert!(
            self.total_staked_balance > 0,
            "The contract doesn't have staked balance"
        );
        // Calculate the number of shares required to unstake the given amount.
        // NOTE: The number of shares the account will pay is rounded up.
        let num_shares = self.num_shares_from_staked_amount_rounded_up(amount);
        assert!(
            num_shares > 0,
            "Invariant violation. The calculated number of \"stake\" shares for unstaking should be positive"
        );
        assert!(
            account.stake_shares >= num_shares,
            "Not enough staked balance to unstake"
        );

        // Calculating the amount of tokens the account will receive by unstaking the corresponding
        // number of "stake" shares, rounding up.
        let receive_amount = self.staked_amount_from_num_shares_rounded_up(num_shares);
        assert!(
            receive_amount > 0,
            "Invariant violation. Calculated staked amount must be positive, because \"stake\" share price should be at least 1"
        );

        account.stake_shares -= num_shares;
        account.unstaked += receive_amount;
        account.unstaked_available_epoch_height = env::epoch_height() + NUM_EPOCHS_TO_UNLOCK;
        self.internal_save_account(&account_id, &account);

        // The amount tokens that will be unstaked from the total to guarantee the "stake" share
        // price never decreases. The difference between `receive_amount` and `unstake_amount` is
        // paid from the allocated STAKE_SHARE_PRICE_GUARANTEE_FUND.
        let unstake_amount = self.staked_amount_from_num_shares_rounded_down(num_shares);

        self.total_staked_balance -= unstake_amount;
        self.total_stake_shares -= num_shares;
        if account.is_burn_account {
            self.total_burn_shares -= num_shares;
        }

        log!(
            "@{} unstaking {}. Spent {} staking shares. Total {} unstaked balance and {} \
             staking shares",
            account_id,
            receive_amount,
            num_shares,
            account.unstaked,
            account.stake_shares
        );
        log!(
            "Contract total staked balance is {}. Total number of shares {}",
            self.total_staked_balance,
            self.total_stake_shares
        );
    }

    pub(crate) fn internal_unstake_all(&mut self, account_id: &AccountId) {
        // Unstake action always restakes
        self.internal_ping();

        let account = self.internal_get_account(&account_id);
        let amount = self.staked_amount_from_num_shares_rounded_down(account.stake_shares);
        self.inner_unstake(account_id, amount);

        self.internal_restake();
    }

    /// Add given number of staked shares to the given account.
    fn internal_add_shares(&mut self, account_id: &AccountId, num_shares: NumStakeShares) {
        if num_shares > 0 {
            let mut account = self.internal_get_account(&account_id);
            account.stake_shares += num_shares;
            self.internal_save_account(&account_id, &account);
            // Increasing the total amount of "stake" shares.
            self.total_stake_shares += num_shares;
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
            self.total_staked_balance += remaining_reward;

            // Now buying "stake" shares for the burn.
            let num_burn_shares = self.num_shares_from_staked_amount_rounded_down(burn_fee);
            self.total_burn_shares += num_burn_shares;

            // Now buying "stake" shares for the contract owner at the new share price.
            let num_owner_shares = self.num_shares_from_staked_amount_rounded_down(owners_fee);

            self.internal_add_shares(
                &AccountId::new_unchecked(ZERO_ADDRESS.to_string()),
                num_burn_shares,
            );
            self.internal_add_shares(&StakingContract::internal_get_owner_id(), num_owner_shares);

            // Increasing the total staked balance by the owners fee, no matter whether the owner
            // received any shares or not.
            self.total_staked_balance += owners_fee + burn_fee;

            log!(
                "Epoch {}: Contract received total rewards of {} tokens. \
                 New total staked balance is {}. Total number of shares {}",
                epoch_height,
                total_reward,
                self.total_staked_balance,
                self.total_stake_shares,
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

    /// Returns the number of "stake" shares rounded down corresponding to the given staked balance
    /// amount.
    ///
    /// price = total_staked / total_shares
    /// Price is fixed
    /// (total_staked + amount) / (total_shares + num_shares) = total_staked / total_shares
    /// (total_staked + amount) * total_shares = total_staked * (total_shares + num_shares)
    /// amount * total_shares = total_staked * num_shares
    /// num_shares = amount * total_shares / total_staked
    pub(crate) fn num_shares_from_staked_amount_rounded_down(
        &self,
        amount: Balance,
    ) -> NumStakeShares {
        assert!(
            self.total_staked_balance > 0,
            "The total staked balance can't be 0"
        );
        (U256::from(self.total_stake_shares) * U256::from(amount)
            / U256::from(self.total_staked_balance))
        .as_u128()
    }

    /// Returns the number of "stake" shares rounded up corresponding to the given staked balance
    /// amount.
    ///
    /// Rounding up division of `a / b` is done using `(a + b - 1) / b`.
    pub(crate) fn num_shares_from_staked_amount_rounded_up(
        &self,
        amount: Balance,
    ) -> NumStakeShares {
        assert!(
            self.total_staked_balance > 0,
            "The total staked balance can't be 0"
        );
        ((U256::from(self.total_stake_shares) * U256::from(amount)
            + U256::from(self.total_staked_balance - 1))
            / U256::from(self.total_staked_balance))
        .as_u128()
    }

    /// Returns the staked amount rounded down corresponding to the given number of "stake" shares.
    pub(crate) fn staked_amount_from_num_shares_rounded_down(
        &self,
        num_shares: NumStakeShares,
    ) -> Balance {
        assert!(
            self.total_stake_shares > 0,
            "The total number of stake shares can't be 0"
        );
        (U256::from(self.total_staked_balance) * U256::from(num_shares)
            / U256::from(self.total_stake_shares))
        .as_u128()
    }

    /// Returns the staked amount rounded up corresponding to the given number of "stake" shares.
    ///
    /// Rounding up division of `a / b` is done using `(a + b - 1) / b`.
    pub(crate) fn staked_amount_from_num_shares_rounded_up(
        &self,
        num_shares: NumStakeShares,
    ) -> Balance {
        assert!(
            self.total_stake_shares > 0,
            "The total number of stake shares can't be 0"
        );
        ((U256::from(self.total_staked_balance) * U256::from(num_shares)
            + U256::from(self.total_stake_shares - 1))
            / U256::from(self.total_stake_shares))
        .as_u128()
    }

    /// Inner method to get the given account or a new default value account.
    pub(crate) fn internal_get_account(&self, account_id: &AccountId) -> Account {
        let mut account = self.accounts.get(account_id).unwrap_or_default();
        account.is_burn_account = account_id.as_str() == ZERO_ADDRESS;
        account
    }

    /// Inner method to save the given account for a given account ID.
    /// If the account balances are 0, the account is deleted instead to release storage.
    pub(crate) fn internal_save_account(&mut self, account_id: &AccountId, account: &Account) {
        if account.unstaked > 0 || account.stake_shares > 0 || account.amounts.len() > 0 {
            self.accounts.insert(account_id, &account);
        } else {
            self.accounts.remove(account_id);
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
