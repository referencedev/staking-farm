use std::collections::HashMap;

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
}

impl Default for Account {
    fn default() -> Self {
        Self {
            unstaked: 0,
            stake_shares: 0,
            unstaked_available_epoch_height: 0,
            last_farm_reward_per_share: HashMap::new(),
            amounts: HashMap::new(),
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