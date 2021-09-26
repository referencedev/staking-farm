use crate::*;
use near_sdk::Timestamp;

const SESSION_INTERVAL: u64 = 1_000_000_000;
const DENOMINATOR: u128 = 1_000_000_000_000_000_000_000_000;

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct RewardDistribution {
    pub undistributed: Balance,
    pub unclaimed: Balance,
    pub rps: U256,
    pub reward_round: u64,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct Farm {
    token_id: AccountId,
    amount: Balance,
    start_date: Timestamp,
    end_date: Timestamp,
    last_distribution: RewardDistribution,
}

impl StakingContract {
    pub(crate) fn internal_deposit_farm_tokens(&mut self, token_id: &AccountId, amount: Balance, start_date: Timestamp, end_date: Timestamp) {
        self.farms.push(&Farm {
            token_id: token_id.clone(), amount, start_date, end_date,
            last_distribution: RewardDistribution { undistributed: amount, unclaimed: 0, rps: U256::zero(), reward_round: 0 }
        });
    }

    fn internal_get_farm(&self, farm_id: u64) -> Farm {
        self.farms.get(farm_id).expect("ERR_NO_FARM")
    }

    fn internal_calculate_distribution(&self, farm: &Farm, total_staked: Balance) -> Option<RewardDistribution> {
        if farm.start_date < env::block_timestamp() {
            // Farm hasn't started.
            return None;
        }
        let mut distribution = farm.last_distribution.clone();
        if distribution.undistributed == 0 {
            // Farm has ended.
            return Some(distribution);
        }
        distribution.reward_round = (env::block_timestamp() - farm.start_date) / SESSION_INTERVAL;
        let reward_per_session = farm.amount * (farm.end_date - farm.start_date) as u128 / SESSION_INTERVAL as u128;
        let mut reward_added = (distribution.reward_round - farm.last_distribution.reward_round) as u128 * reward_per_session;
        if farm.last_distribution.undistributed < reward_added {
            // Last step when the last tokens are getting distributed.
            reward_added = farm.last_distribution.undistributed;
            let increase_reward_round = (reward_added / reward_per_session) as u64;
            distribution.reward_round = farm.last_distribution.reward_round + increase_reward_round;
            if increase_reward_round as u128 * reward_per_session < reward_added {
                // Fix the rounding.
                distribution.reward_round += 1;
            }
        }
        distribution.unclaimed += reward_added;
        distribution.undistributed -= reward_added;

        if total_staked == 0 {
            distribution.rps = U256::zero();
        } else {
            distribution.rps = farm.last_distribution.rps + U256::from(reward_added * DENOMINATOR / total_staked);
        }
        Some(distribution)
    }

     fn internal_unclaimed_balance(&self, account: &Account, farm_id: u64, farm: &Farm) -> (U256, Balance) {
         let user_rps = account.user_rps.get(&farm_id).cloned().unwrap_or(U256::zero());
         if let Some(distribution) = self.internal_calculate_distribution(&farm, self.total_stake_shares) {
             (farm.last_distribution.rps, (U256::from(account.stake_shares) * (distribution.rps - user_rps) / DENOMINATOR).as_u128())
         } else {
             (U256::zero(), 0)
         }
     }

    fn internal_distribute(&mut self, farm_id: u64) {
        let mut farm = self.internal_get_farm(farm_id);
        if let Some(distribution) = self.internal_calculate_distribution(&farm, self.total_stake_shares) {
            if distribution.reward_round != farm.last_distribution.reward_round {
                farm.last_distribution = distribution;
            }
            self.farms.replace(farm_id, &farm);
        }
    }

    fn internal_claim_reward(&mut self, account_id: &AccountId, farm_id: u64) {
        let farm = self.internal_get_farm(farm_id);
        let mut account = self.internal_get_account(account_id);
        let (new_user_rps, claim_amount) = self.internal_unclaimed_balance(&account, farm_id, &farm);
        account.user_rps.insert(farm_id, new_user_rps);
        if claim_amount > 0 {
            // TODO: send the tokens to the user.
        }
        self.accounts.insert(account_id, &account);
    }
}

#[near_bindgen]
impl StakingContract {
    /// Claim tokens from the given farm for the caller.
    pub fn claim(&mut self, farm_id: u64) {

    }
}