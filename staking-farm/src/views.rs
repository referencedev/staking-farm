use near_sdk::json_types::{U128, U64};
use near_sdk::{env, AccountId};

use crate::internal::ZERO_ADDRESS;
use crate::Farm;
use crate::*;

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableFarm {
    pub farm_id: u64,
    pub name: String,
    pub token_id: AccountId,
    pub amount: U128,
    pub start_date: U64,
    pub end_date: U64,
    pub active: bool,
}

impl HumanReadableFarm {
    fn from(farm_id: u64, farm: Farm) -> Self {
        let active = farm.is_active();
        HumanReadableFarm {
            farm_id,
            name: farm.name,
            token_id: farm.token_id,
            amount: U128(farm.amount),
            start_date: U64(farm.start_date),
            end_date: U64(farm.end_date),
            active,
        }
    }
}

/// Represents an account structure readable by humans.
#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableAccount {
    pub account_id: AccountId,
    /// The unstaked balance that can be withdrawn or staked.
    pub unstaked_balance: U128,
    /// The amount balance staked at the current "stake" share price.
    pub staked_balance: U128,
    /// Whether the unstaked balance is available for withdrawal now.
    pub can_withdraw: bool,
    /// Rewards showing information for those accounts
    /// that have their tokens delegated to a pool which
    /// doesnt restake its rewards
    pub rewards_for_withdraw: U128,
}

/// Represents pool summary with all farms and rates applied.
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct PoolSummary {
    /// Pool owner.
    pub owner: AccountId,
    /// The total staked balance.
    pub total_staked_balance: U128,
    /// The total amount to burn that will be available
    /// The fraction of the reward that goes to the owner of the staking pool for running the
    /// validator node.
    pub reward_fee_fraction: Ratio,
    /// If reward fee fraction is changing, this will be different from current.
    pub next_reward_fee_fraction: Ratio,
    /// The fraction of the reward that gets burnt.
    pub burn_fee_fraction: Ratio,
    /// Active farms that affect stakers.
    pub farms: Vec<HumanReadableFarm>,
}

#[near_bindgen]
impl StakingContract {
    /// Returns summary of this pool.
    /// Can calculate rate of return of this pool with farming by:
    /// `farm_reward_per_day = farms.iter().map(farms.amount / (farm.end_date - farm.start_date) / DAY_IN_NS * PRICES[farm.token_id]).sum()`
    /// `near_reward_per_day = total_near_emission_per_day * this.total_staked_balance / total_near_staked * (1 - this.burn_fee_fraction) * (1 - this.reward_fee_fraction)`
    /// `total_reward_per_day = farm_reward_per_day + near_reward_per_day * NEAR_PRICE`
    /// `reward_rate = total_reward_per_day / (this.total_staked_balance * NEAR_PRICE)`
    pub fn get_pool_summary(&self) -> PoolSummary {
        PoolSummary {
            owner: StakingContract::internal_get_owner_id(),
            total_staked_balance: (self.rewards_staked_staking_pool.total_staked_balance + self.rewards_not_staked_staking_pool.total_staked_balance).into(),
            reward_fee_fraction: self.reward_fee_fraction.current().clone(),
            next_reward_fee_fraction: self.reward_fee_fraction.next().clone(),
            burn_fee_fraction: self.burn_fee_fraction.clone(),
            farms: self.get_active_farms(),
        }
    }

    ///
    /// OWNER
    ///

    /// Returns current contract version.
    pub fn get_version(&self) -> String {
        Self::internal_get_version()
    }

    /// Returns current owner from the storage.
    pub fn get_owner_id(&self) -> AccountId {
        Self::internal_get_owner_id()
    }

    /// Returns current contract factory.
    pub fn get_factory_id(&self) -> AccountId {
        Self::internal_get_factory_id()
    }

    /// Return all authorized users.
    pub fn get_authorized_users(&self) -> Vec<AccountId> {
        self.authorized_users.to_vec()
    }

    /// Return all authorized tokens.
    pub fn get_authorized_farm_tokens(&self) -> Vec<AccountId> {
        self.authorized_farm_tokens.to_vec()
    }

    ///
    /// FARMS
    ///

    pub fn get_active_farms(&self) -> Vec<HumanReadableFarm> {
        self.active_farms
            .iter()
            .map(|&index| HumanReadableFarm::from(index, self.farms.get(index).unwrap()))
            .collect()
    }

    pub fn get_farms(&self, from_index: u64, limit: u64) -> Vec<HumanReadableFarm> {
        (from_index..std::cmp::min(from_index + limit, self.farms.len()))
            .map(|index| HumanReadableFarm::from(index, self.farms.get(index).unwrap()))
            .collect()
    }

    pub fn get_farm(&self, farm_id: u64) -> HumanReadableFarm {
        HumanReadableFarm::from(farm_id, self.internal_get_farm(farm_id))
    }

    pub fn get_unclaimed_reward(&self, account_id: AccountId, farm_id: u64) -> U128 {
        if account_id == AccountId::new_unchecked(ZERO_ADDRESS.to_string()) {
            return U128(0);
        }
        let staking_pool = self.get_staking_pool_or_default(&account_id);
        
        let account = staking_pool.get_account_impl(&account_id);
        let mut farm = self.farms.get(farm_id).expect("ERR_NO_FARM");
        let (_rps, reward) = self.internal_unclaimed_balance(account.as_ref(), farm_id, &mut farm, staking_pool.does_pool_stake_staking_rewards());
        let prev_reward = account.get_farm_amount(farm.token_id);
        U128(reward + prev_reward)
    }

    ///
    /// ACCOUNT
    ///
    
    ///  /// Returns the rewards for the account, If the account is in the
    /// staking pool that doesnt restake its rewards it will return somethind
    /// If is in the other pool it will return 0
    pub fn get_account_not_staked_rewards(&self, account_id: AccountId) -> U128{
        self.get_account(account_id).rewards_for_withdraw
    }

    /// Returns the unstaked balance of the given account.
    pub fn get_account_unstaked_balance(&self, account_id: AccountId) -> U128 {
        self.get_account(account_id).unstaked_balance
    }

    /// Returns the staked balance of the given account.
    /// NOTE: This is computed from the amount of "stake" shares the given account has and the
    /// current amount of total staked balance and total stake shares on the account.
    pub fn get_account_staked_balance(&self, account_id: AccountId) -> U128 {
        self.get_account(account_id).staked_balance
    }

    /// Returns the total balance of the given account (including staked and unstaked balances).
    pub fn get_account_total_balance(&self, account_id: AccountId) -> U128 {
        let account = self.get_account(account_id);
        (account.unstaked_balance.0 + account.staked_balance.0).into()
    }

    /// Returns `true` if the given account can withdraw tokens in the current epoch.
    pub fn is_account_unstaked_balance_available(&self, account_id: AccountId) -> bool {
        self.get_account(account_id).can_withdraw
    }

    /// Returns the total staking balance.
    pub fn get_total_staked_balance(&self) -> U128 {
        (self.rewards_staked_staking_pool.total_staked_balance 
            + self.rewards_not_staked_staking_pool.total_staked_balance)
            .into()
    }

    /// Returns the current reward fee as a fraction.
    pub fn get_reward_fee_fraction(&self) -> Ratio {
        self.reward_fee_fraction.current().clone()
    }

    /// Returns the staking public key
    pub fn get_staking_key(&self) -> PublicKey {
        self.stake_public_key.clone().try_into().unwrap()
    }

    /// Returns true if the staking is paused
    pub fn is_staking_paused(&self) -> bool {
        self.paused
    }

    /// Returns human readable representation of the account for the given account ID.
    pub fn get_account(&self, account_id: AccountId) -> HumanReadableAccount {
        let staking_pool = self.get_staking_pool_or_default(&account_id);
        return staking_pool.get_account_info(&account_id);
    }

    /// Returns the number of accounts that have positive balance on this staking pool.
    pub fn get_number_of_accounts(&self) -> u64 {
        self.rewards_staked_staking_pool.accounts.len() + self.rewards_not_staked_staking_pool.accounts.len()
    }

    /// Returns the list of accounts
    pub fn get_accounts(&self, from_index: u64, limit: u64) -> Vec<HumanReadableAccount> {
        let keys = self
            .rewards_staked_staking_pool
            .accounts
            .keys_as_vector();
        let upper_bound = std::cmp::min(from_index + limit, keys.len());

        let mut result = (from_index..upper_bound)
            .map(|index| self.get_account(keys.get(index).unwrap()))
            .collect::<Vec<HumanReadableAccount>>();

        if upper_bound - (result.len() as u64) > 0 {
            let other_keys = self
                .rewards_not_staked_staking_pool
                .accounts
                .keys_as_vector();
                
            let other_upper_bound = std::cmp::min(upper_bound - (result.len() as u64), other_keys.len());
            let other_result = (0..other_upper_bound)
                .map(|index| self.get_account(other_keys.get(index).unwrap()))
                .collect::<Vec<HumanReadableAccount>>();
            result.extend(other_result);
        }

        return result;
    }
}
