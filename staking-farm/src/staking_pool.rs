use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, log, Balance, AccountId};
use near_sdk::collections::UnorderedMap;
use crate::account::{Account, AccountWithReward, NumStakeShares};
use crate::StorageKeys;
use uint::construct_uint;
use crate::views::HumanReadableAccount;
construct_uint! {
    /// 256-bit unsigned integer.
    pub struct U256(4);
}

pub trait StakingPool{
    fn get_total_staked_balance(&self) -> Balance;
    fn get_account_info(&self, account_id: &AccountId) -> HumanReadableAccount;
    fn deposit(&mut self, account_id: &AccountId, amount: Balance);
    fn withdraw(&mut self, account_id: &AccountId, amount: Balance) -> bool;
    /*
    fn stake(&mut self, account_id: &AccountId, amount: Balance);
    fn unstake(&mut self, account_id: &AccountId, amount: Balance);
    fn get_account_unstaked_balance(&self, account_id: &AccountId) -> Balance;
    
    */
    /// send rewards to receiver account id
    /// and remove account from account pool register if needed
    /// returns amount to send and flag indicating wether an account should be removed
    /// from register
    fn withdraw_not_staked_rewards(&mut self, account_id: &AccountId) -> (Balance, bool);
}

/// Structure containing information for accounts that have their rewards restaked
#[derive(BorshDeserialize, BorshSerialize)]
pub struct InnerStakingPool{
    /// The total amount of shares the staking pool has across the contract
    pub total_stake_shares: NumStakeShares,
    /// The total staked balance.
    pub total_staked_balance: Balance,
    /// Persistent map from an account ID to the corresponding account.
    pub accounts: UnorderedMap<AccountId, Account>,
}

/// Structure containing information for accounts that have their rewards not being restaked
#[derive(BorshDeserialize, BorshSerialize)]
pub struct InnerStakingPoolWithoutRewardsRestaked{
    pub accounts: UnorderedMap<AccountId, AccountWithReward>,
    /// Pool total staked balance
    pub total_staked_balance: Balance,
    /// Accounts deposit, it would be used when calculating how much of the total rewards is for each account
    /// and also how much of the total staked balance can be unstaked
    pub reward_per_token: Fraction,
}

#[derive(BorshDeserialize, BorshSerialize, PartialEq, Eq, Debug)]
pub struct Fraction{
    pub numerator: u128,
    pub denominator: u128,
}

impl Default for Fraction{
    fn default() -> Self {
        Self {
            numerator: 0,
            denominator: 0,
        }
    }
}

impl Fraction{
    pub fn new(
        numerator: u128, 
        denominator: u128
    )-> Self{
        assert!((denominator == 0 && numerator == 0) 
        || (denominator != 0 && numerator != 0), "Denominator can only be 0 if numerator is 0");

        return Self{
            numerator: numerator,
            denominator: denominator
        };
    }
    pub fn add(&mut self, value: Fraction)-> &mut Self{
        if value == Fraction::default(){
            //do nothing
        }else if *self == Fraction::default(){
            self.numerator = value.numerator;
            self.denominator = value.denominator;
        }else {   
            // Finding greatest common divisor of the two denominators
            let gcd = self.greatest_common_divisior(self.denominator,value.denominator);      
            let new_denominator = ((U256::from(self.denominator) * U256::from(value.denominator)) / U256::from(gcd)).as_u128();
        
            // Changing the fractions to have same denominator
            // Numerator of the final fraction obtained
            self.numerator = (self.numerator) * (new_denominator / self.denominator) 
                    + (value.numerator) * (new_denominator / value.denominator);
            self.denominator = new_denominator;
        }
        // Calling function to convert final fraction
        // into it's simplest form
        self.simple_form();

        return self;
    }

    pub fn multiply(&self, value: Balance) -> Balance {
        if self.numerator == 0 && self.denominator == 0 {
            return 0;
        }

        return (U256::from(self.numerator) * U256::from(value) / U256::from(self.denominator)).as_u128()
    }

    fn simple_form(&mut self) -> &Self{
        if *self == Fraction::default(){
            return self;
        }
        let common_factor = self.greatest_common_divisior(self.numerator, self.denominator);
        self.denominator = self.denominator/common_factor;
        self.numerator = self.numerator/common_factor;

        return self;
    }

    fn greatest_common_divisior(
        &self, 
        a: u128, 
        b: u128
    ) -> u128{
        if a == 0{
            return b;
        }
        return self.greatest_common_divisior(b%a, a);
    }
}

impl InnerStakingPool{
    /// Constructor
    pub fn new(
        stake_shares: NumStakeShares,
        staked_balance: Balance
    ) -> Self{
        let this = Self{
            accounts: UnorderedMap::new(StorageKeys::Accounts),
            total_stake_shares: stake_shares,
            total_staked_balance: staked_balance
        };

        return this;
    }

    /// Inner method to get the given account or a new default value account.
    pub(crate) fn internal_get_account(&self, account_id: &AccountId) -> Account {
        self.accounts.get(account_id).unwrap_or_default()
    }

    /// Inner method to save the given account for a given account ID.
    /// If the account balances are 0, the account is deleted instead to release storage.
    /// Returns true or false wether the account was removed
    pub(crate) fn internal_save_account(&mut self, account_id: &AccountId, account: &Account) -> bool {
        if account.unstaked > 0 || account.stake_shares > 0 || account.amounts.len() > 0 {
            self.accounts.insert(account_id, &account);
            return false;
        } else {
            self.accounts.remove(account_id);
            return true;
        }
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
}

impl InnerStakingPoolWithoutRewardsRestaked{
    /// Constructor
    pub fn new() -> Self{
        return Self {
            reward_per_token: Fraction::new(0, 0),
            total_staked_balance: 0,
            accounts: UnorderedMap::new(StorageKeys::AccountsNotStakedStakingPool),
        };
    }

    /// Inner method to get the given account or a new default value account.
    pub(crate) fn internal_get_account(&self, account_id: &AccountId) -> AccountWithReward {
        self.accounts.get(account_id).unwrap_or_default()
    }

    /// Inner method to save the given account for a given account ID.
    /// If the account balances are 0, the account is deleted instead to release storage.
    /// Returns true or false, wether the account was removeds
    pub(crate) fn internal_save_account(&mut self, account_id: &AccountId, account: &AccountWithReward) -> bool{
        if account.unstaked > 0 || account.stake > 0 || account.amounts.len() > 0 || account.reward_tally > 0 {
            self.accounts.insert(account_id, &account);
            return false;
        } else {
            self.accounts.remove(account_id);
            return true;
        }
    }

    pub(crate) fn distribute_reward(&mut self, reward:Balance){
        if reward == 0{
            return;
        }
        assert!(self.total_staked_balance > 0, "Cannot distribute reward when staked balance is 0 or below");
        self.reward_per_token.add(Fraction::new(reward, self.total_staked_balance));
    }

    pub(crate) fn compute_reward(&self, account: &AccountWithReward) -> Balance{
        if account.tally_below_zero {
            return self.reward_per_token.multiply(account.stake) + account.reward_tally;
        }else{
            return self.reward_per_token.multiply(account.stake) - account.reward_tally;
        }
    }
}

impl StakingPool for InnerStakingPool{
    fn get_total_staked_balance(&self) -> Balance {
        return self.total_staked_balance;
    }

    fn get_account_info(&self, account_id: &AccountId) -> HumanReadableAccount {
        let account = self.internal_get_account(&account_id);
        return HumanReadableAccount {
            account_id: account_id.clone(),
            unstaked_balance: account.unstaked.into(),
            staked_balance: self
                .staked_amount_from_num_shares_rounded_down(account.stake_shares)
                .into(),
            can_withdraw: account.unstaked_available_epoch_height <= env::epoch_height(),
            rewards_for_withdraw: 0.into()
        }
    }

    fn deposit(&mut self, account_id: &AccountId, amount: Balance) {
        let mut account = self.internal_get_account(&account_id);
        
        account.unstaked += amount;
        self.internal_save_account(&account_id, &account);

        log!(
            "@{} deposited {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );
    }

    fn withdraw_not_staked_rewards(&mut self, account_id: &AccountId) -> (Balance, bool){
        return (0, false);
    }

    fn withdraw(&mut self, account_id: &AccountId, amount: Balance) -> bool{
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
        let account_has_been_removed = self.internal_save_account(&account_id, &account);

        log!(
            "@{} withdrawing {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );

        return account_has_been_removed;
    }
}

impl StakingPool for InnerStakingPoolWithoutRewardsRestaked{
    fn get_total_staked_balance(&self) -> Balance {
        return self.total_staked_balance;
    }

    fn get_account_info(&self, account_id: &AccountId) -> HumanReadableAccount {
        let account = self.internal_get_account(&account_id);
        return HumanReadableAccount {
            account_id: account_id.clone(),
            unstaked_balance: account.unstaked.into(),
            staked_balance: account.stake.into(),
            can_withdraw: account.unstaked_available_epoch_height <= env::epoch_height(),
            rewards_for_withdraw: self.compute_reward(&account).into(),
        };
    }

    fn deposit(&mut self, account_id: &AccountId, amount: Balance) {
        let mut account = self.internal_get_account(&account_id);
        
        account.unstaked += amount;
        self.internal_save_account(&account_id, &account);

        log!(
            "@{} deposited {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );
    }

    fn withdraw_not_staked_rewards(&mut self, account_id: &AccountId) -> (Balance, bool){
        let mut account = self.internal_get_account(&account_id);
        let reward = self.compute_reward(&account);
        account.reward_tally = self.reward_per_token.multiply(account.stake);
        let account_was_removed = self.internal_save_account(&account_id, &account);

        return (reward, account_was_removed);
    }

    fn withdraw(&mut self, account_id: &AccountId, amount: Balance) -> bool{
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
        let account_has_been_removed = self.internal_save_account(&account_id, &account);

        log!(
            "@{} withdrawing {}. New unstaked balance is {}",
            account_id,
            amount,
            account.unstaked
        );

        return account_has_been_removed;
    }
}