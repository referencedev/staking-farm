use std::collections::HashMap;
use std::any::Any;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, Balance, EpochHeight};

use crate::U256;

/// A type to distinguish between a balance and "stake" shares for better readability.
pub type NumStakeShares = Balance;

/// Inner account data of a delegate.
#[derive(BorshDeserialize, BorshSerialize, Debug, PartialEq)]
pub struct Account {
    /// The unstaked balance. It represents the amount the account has on this contract that
    /// can either be staked or withdrawn.
    pub unstaked: Balance,
    /// The amount of "stake" shares. Every stake share corresponds to the amount of staked balance.
    /// NOTE: The number of shares should always be less or equal than the amount of staked balance.
    /// This means the price of stake share should always be at least `1`.
    /// The price of stake share can be computed as `total_staked_balance` / `total_stake_shares`.
    pub stake_shares: NumStakeShares,
    /// The minimum epoch height when the withdrawn is allowed.
    /// This changes after unstaking action, because the amount is still locked for 3 epochs.
    pub unstaked_available_epoch_height: EpochHeight,
    /// Last claimed reward for each active farm.
    pub last_farm_reward_per_share: HashMap<u64, U256>,
    /// Farmed tokens withdrawn from the farm but not from the contract.
    pub amounts: HashMap<AccountId, Balance>,
    /// Is this a burn account.
    /// Note: It's not persisted in the state, but initialized during internal_get_account.
    #[borsh_skip]
    pub is_burn_account: bool,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            unstaked: 0,
            stake_shares: 0,
            unstaked_available_epoch_height: 0,
            last_farm_reward_per_share: HashMap::new(),
            amounts: HashMap::new(),
            is_burn_account: false,
        }
    }
}

/// Inner account data of a delegate.
#[derive(BorshDeserialize, BorshSerialize, Debug, PartialEq)]
pub struct AccountWithReward{
    /// The unstaked balance. It represents the amount the account has on this contract that
    /// can either be staked or withdrawn.
    pub unstaked: Balance,
    /// The amount of "stake" shares. Every stake share corresponds to the amount of staked balance.
    /// NOTE: The number of shares should always be less or equal than the amount of staked balance.
    /// This means the price of stake share should always be at least `1`.
    /// The price of stake share can be computed as `total_staked_balance` / `total_stake_shares`.
    pub stake: Balance,
    /// The minimum epoch height when the withdrawn is allowed.
    /// This changes after unstaking action, because the amount is still locked for 3 epochs.
    pub unstaked_available_epoch_height: EpochHeight,
    /// The reward that has been paid to the account
    pub reward_tally: Balance,
    /// Bool variable showing whether the reward_tally is positive or negative
    pub tally_below_zero: bool,
    /// Last claimed reward for each active farm.
    pub last_farm_reward_per_share: HashMap<u64, U256>,
    /// Farmed tokens withdrawn from the farm but not from the contract.
    pub amounts: HashMap<AccountId, Balance>,
}

impl Default for AccountWithReward {
    fn default() -> Self {
        Self {
            unstaked: 0,
            stake: 0,
            unstaked_available_epoch_height: 0,
            reward_tally: 0,
            tally_below_zero: false,
            last_farm_reward_per_share: HashMap::new(),
            amounts: HashMap::new(),
        }
    }
}

impl AccountWithReward{
    pub fn add_to_tally(&mut self, amount: Balance){
        if self.tally_below_zero{
            if amount >= self.reward_tally {
                self.reward_tally = amount - self.reward_tally;
                self.tally_below_zero = !self.tally_below_zero; 
            }else{
                self.reward_tally -= amount;
            }
        }else{
            self.reward_tally += amount;
        }
    }

    pub fn subtract_from_tally(&mut self, amount: Balance){
        if self.tally_below_zero{
            self.reward_tally += amount;            
        }else{
            if amount > self.reward_tally {
                self.reward_tally = amount - self.reward_tally;
                self.tally_below_zero = !self.tally_below_zero; 
            }else{
                self.reward_tally -= amount;
            }
        }
    }
}

pub trait AccountImpl{
    fn update_last_farm_reward_per_share(&mut self, farm_id: u64, rps: U256);
    fn get_account_stake_shares(&self) -> NumStakeShares;
    fn update_farm_amounts(&mut self, farm_token_id: AccountId, claim_amount: Balance);
    fn get_last_reward_per_share(&self, farm_id: u64) -> U256;
    fn get_farm_amount(&self, farm_token_id: AccountId) -> Balance;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_burn_account(&self) -> bool;
}

impl AccountImpl for Account{
    fn as_any(&self) -> &dyn Any {
        return self;
    }

    fn is_burn_account(&self) -> bool {
        return self.is_burn_account;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        return self;
    }

    fn get_farm_amount(&self, farm_token_id: AccountId) -> Balance {
        return *self.amounts.get(&farm_token_id).unwrap_or(&0);
    }

    fn get_account_stake_shares(&self) -> NumStakeShares{
        return self.stake_shares;
    }

    fn get_last_reward_per_share(&self, farm_id: u64) -> U256{
        return self
            .last_farm_reward_per_share
            .get(&farm_id)
            .cloned()
            .unwrap_or(U256::zero());
    }

    fn update_last_farm_reward_per_share(&mut self, farm_id: u64, rps: U256){
        self
            .last_farm_reward_per_share
            .insert(farm_id, rps);
    }

    fn update_farm_amounts(&mut self, farm_token_id: AccountId, claim_amount: Balance){
        *self.amounts.entry(farm_token_id).or_default() += claim_amount;
    }
}

impl AccountImpl for AccountWithReward{
    fn get_account_stake_shares(&self) -> NumStakeShares{
        return self.stake;
    }

    fn is_burn_account(&self) -> bool {
        return false;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        return self;
    }

    fn as_any(&self) -> &dyn Any {
        return self;
    }

    fn get_farm_amount(&self, farm_token_id: AccountId) -> Balance {
        return *self.amounts.get(&farm_token_id).unwrap_or(&0);
    }

    fn get_last_reward_per_share(&self, farm_id: u64) -> U256{
        return self
            .last_farm_reward_per_share
            .get(&farm_id)
            .cloned()
            .unwrap_or(U256::zero());
    }

    fn update_last_farm_reward_per_share(&mut self, farm_id: u64, rps: U256){
        self
            .last_farm_reward_per_share
            .insert(farm_id, rps);
    }

    fn update_farm_amounts(&mut self, farm_token_id: AccountId, claim_amount: Balance){
        *self.amounts.entry(farm_token_id).or_default() += claim_amount;
    }
}