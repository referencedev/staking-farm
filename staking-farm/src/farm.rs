use near_contract_standards::fungible_token::core_impl::ext_fungible_token;
use near_sdk::{assert_one_yocto, is_promise_success, promise_result_as_success, Timestamp};

use crate::stake::ext_self;
use crate::*;

const SESSION_INTERVAL: u64 = 1_000_000_000;
const DENOMINATOR: u128 = 1_000_000_000_000_000_000_000_000;

/// Amount of gas for fungible token transfers.
pub const GAS_FOR_FT_TRANSFER: Gas = Gas(10_000_000_000_000);
/// hotfix_insuffient_gas_for_mft_resolve_transfer, increase from 5T to 20T
pub const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(20_000_000_000_000);
/// Gas for calling `get_owner` method.
pub const GAS_FOR_GET_OWNER: Gas = Gas(10_000_000_000_000);
pub const GAS_LEFTOVERS: Gas = Gas(20_000_000_000_000);
/// Get owner method on external contracts.
pub const GET_OWNER_METHOD: &str = "get_owner_account_id";

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct RewardDistribution {
    pub undistributed: Balance,
    pub unclaimed: Balance,
    pub reward_per_share: U256,
    pub reward_round: u64,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct Farm {
    pub name: String,
    pub token_id: AccountId,
    pub amount: Balance,
    pub start_date: Timestamp,
    pub end_date: Timestamp,
    pub last_distribution: RewardDistribution,
}

impl Farm {
    pub fn is_active(&self) -> bool {
        self.last_distribution.undistributed > 0
    }
}

impl StakingContract {
    pub(crate) fn internal_deposit_farm_tokens(
        &mut self,
        token_id: &AccountId,
        name: String,
        amount: Balance,
        start_date: Timestamp,
        end_date: Timestamp,
    ) {
        assert!(start_date >= env::block_timestamp(), "ERR_FARM_TOO_EARLY");
        assert!(end_date > start_date + SESSION_INTERVAL, "ERR_FARM_DATE");
        assert!(amount > 0, "ERR_FARM_AMOUNT_NON_ZERO");
        assert!(
            amount / ((end_date - start_date) / SESSION_INTERVAL) as u128 > 0,
            "ERR_FARM_AMOUNT_TOO_SMALL"
        );
        self.farms.push(&Farm {
            name,
            token_id: token_id.clone(),
            amount,
            start_date,
            end_date,
            last_distribution: RewardDistribution {
                undistributed: amount,
                unclaimed: 0,
                reward_per_share: U256::zero(),
                reward_round: 0,
            },
        });
        self.active_farms.push(self.farms.len() - 1);
    }

    pub(crate) fn internal_get_farm(&self, farm_id: u64) -> Farm {
        self.farms.get(farm_id).expect("ERR_NO_FARM")
    }

    fn internal_calculate_distribution(
        &self,
        farm: &Farm,
        total_staked: Balance,
    ) -> Option<RewardDistribution> {
        if farm.start_date > env::block_timestamp() {
            // Farm hasn't started.
            return None;
        }
        let mut distribution = farm.last_distribution.clone();
        if distribution.undistributed == 0 {
            // Farm has ended.
            return Some(distribution);
        }
        distribution.reward_round = (env::block_timestamp() - farm.start_date) / SESSION_INTERVAL;
        let reward_per_session =
            farm.amount / (farm.end_date - farm.start_date) as u128 * SESSION_INTERVAL as u128;
        let mut reward_added = (distribution.reward_round - farm.last_distribution.reward_round)
            as u128
            * reward_per_session;
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
            distribution.reward_per_share = U256::zero();
        } else {
            distribution.reward_per_share = farm.last_distribution.reward_per_share
                + U256::from(reward_added) * U256::from(DENOMINATOR) / U256::from(total_staked);
        }
        Some(distribution)
    }

    pub(crate) fn internal_unclaimed_balance(
        &self,
        account: &Account,
        farm_id: u64,
        farm: &mut Farm,
    ) -> (U256, Balance) {
        if let Some(distribution) = self.internal_calculate_distribution(
            &farm,
            self.total_stake_shares - self.total_burn_shares,
        ) {
            if distribution.reward_round != farm.last_distribution.reward_round {
                farm.last_distribution = distribution.clone();
            }
            let user_rps = account
                .last_farm_reward_per_share
                .get(&farm_id)
                .cloned()
                .unwrap_or(U256::zero());
            (
                farm.last_distribution.reward_per_share,
                (U256::from(account.stake_shares) * (distribution.reward_per_share - user_rps)
                    / DENOMINATOR)
                    .as_u128(),
            )
        } else {
            (U256::zero(), 0)
        }
    }

    fn internal_distribute_reward(
        &mut self,
        account: &mut Account,
        farm_id: u64,
        mut farm: &mut Farm,
    ) {
        let (new_user_rps, claim_amount) =
            self.internal_unclaimed_balance(&account, farm_id, &mut farm);
        account
            .last_farm_reward_per_share
            .insert(farm_id, new_user_rps);
        *account.amounts.entry(farm.token_id.clone()).or_default() += claim_amount;
        env::log_str(&format!(
            "Record {} {} reward from farm #{}",
            claim_amount, farm.token_id, farm_id
        ));
    }

    /// Distribute all rewards for the given user.
    pub(crate) fn internal_distribute_all_rewards(&mut self, mut account: &mut Account) {
        let old_active_farms = self.active_farms.clone();
        self.active_farms = vec![];
        for farm_id in old_active_farms.into_iter() {
            if let Some(mut farm) = self.farms.get(farm_id) {
                self.internal_distribute_reward(&mut account, farm_id, &mut farm);
                self.farms.replace(farm_id, &farm);
                // TODO: currently all farms continue to be active.
                // if farm.is_active() {
                self.active_farms.push(farm_id);
                // }
            }
        }
    }

    fn internal_user_token_deposit(
        &mut self,
        account_id: &AccountId,
        token_id: &AccountId,
        amount: Balance,
    ) {
        let mut account = self.internal_get_account(&account_id);
        *account.amounts.entry(token_id.clone()).or_default() += amount;
        self.internal_save_account(&account_id, &account);
    }

    fn internal_claim(
        &mut self,
        token_id: &AccountId,
        claim_account_id: &AccountId,
        send_account_id: &AccountId,
    ) -> Promise {
        let mut account = self.internal_get_account(&claim_account_id);
        self.internal_distribute_all_rewards(&mut account);
        let amount = account.amounts.remove(&token_id).unwrap_or(0);
        assert!(amount > 0, "ERR_ZERO_AMOUNT");
        env::log_str(&format!(
            "{} receives {} of {} from {}",
            send_account_id, amount, token_id, claim_account_id
        ));
        self.internal_save_account(&claim_account_id, &account);
        ext_fungible_token::ft_transfer(
            send_account_id.clone(),
            U128(amount),
            None,
            token_id.clone(),
            1,
            GAS_FOR_FT_TRANSFER,
        )
        .then(ext_self::callback_post_withdraw_reward(
            token_id.clone(),
            // Return funds to the account that was deducted from vs caller.
            claim_account_id.clone(),
            U128(amount),
            env::current_account_id(),
            0,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
    }
}

#[near_bindgen]
impl StakingContract {
    /// Callback after checking owner for the delegated claim.
    #[private]
    pub fn callback_post_get_owner(
        &mut self,
        token_id: AccountId,
        delegator_id: AccountId,
        account_id: AccountId,
    ) -> Promise {
        let owner_id: AccountId = near_sdk::serde_json::from_slice(
            &promise_result_as_success().expect("get_owner must have result"),
        )
        .expect("Failed to parse");
        assert_eq!(owner_id, account_id, "Caller is not an owner");
        self.internal_claim(&token_id, &delegator_id, &account_id)
    }

    /// Callback from depositing funds to the user's account.
    /// If it failed, return funds to the user's account.
    #[private]
    pub fn callback_post_withdraw_reward(
        &mut self,
        token_id: AccountId,
        sender_id: AccountId,
        amount: U128,
    ) {
        if !is_promise_success() {
            // This reverts the changes from the claim function.
            self.internal_user_token_deposit(&sender_id, &token_id, amount.0);
            env::log_str(&format!(
                "Returned {} {} to {}",
                amount.0, token_id, sender_id
            ));
        }
    }

    /// Claim given tokens for given account.
    /// If delegator is provided, it will call it's `get_owner` method to confirm that caller
    /// can execute on behalf of this contract.
    /// - Requires one yoctoNEAR. To pass to the ft_transfer call and to guarantee the full access key.
    #[payable]
    pub fn claim(&mut self, token_id: AccountId, delegator_id: Option<AccountId>) -> Promise {
        assert_one_yocto();
        let account_id = env::predecessor_account_id();
        if let Some(delegator_id) = delegator_id {
            Promise::new(delegator_id.clone())
                .function_call(GET_OWNER_METHOD.to_string(), vec![], 0, GAS_FOR_GET_OWNER)
                .then(ext_self::callback_post_get_owner(
                    token_id,
                    delegator_id,
                    account_id,
                    env::current_account_id(),
                    0,
                    env::prepaid_gas() - env::used_gas() - GAS_FOR_GET_OWNER - GAS_LEFTOVERS,
                ))
        } else {
            self.internal_claim(&token_id, &account_id, &account_id)
        }
    }

    /// Stops given farm at the current moment.
    /// Warning: IF OWNER ACCOUNT DOESN'T HAVE STORAGE, THESE FUNDS WILL BE STUCK ON THE STAKING FARM.
    pub fn stop_farm(&mut self, farm_id: u64) -> Promise {
        self.assert_owner();
        let mut farm = self.internal_get_farm(farm_id);
        let leftover_amount = (U256::from(farm.amount)
            * U256::from(farm.end_date - env::block_timestamp())
            / U256::from(farm.end_date - farm.start_date))
        .as_u128();
        farm.end_date = env::block_timestamp();
        farm.amount -= leftover_amount;
        farm.last_distribution.undistributed -= leftover_amount;
        self.farms.replace(farm_id, &farm);
        ext_fungible_token::ft_transfer(
            StakingContract::get_owner_id(),
            U128(leftover_amount),
            None,
            farm.token_id.clone(),
            1,
            GAS_FOR_FT_TRANSFER,
        )
    }
}
