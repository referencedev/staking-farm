use near_sdk::AccountId;
use near_sdk::Balance;

pub fn staking() -> AccountId {
    "staking".parse().unwrap()
}

pub fn alice() -> AccountId {
    "alice".parse().unwrap()
}
pub fn bob() -> AccountId {
    "bob".parse().unwrap()
}
pub fn owner() -> AccountId {
    "owner".parse().unwrap()
}
pub fn charlie() -> AccountId {
    "charlie".parse().unwrap()
}

pub fn ntoy(near_amount: Balance) -> Balance {
    near_amount * 10u128.pow(24)
}

/// Rounds to nearest
pub fn yton(yocto_amount: Balance) -> Balance {
    (yocto_amount + (5 * 10u128.pow(23))) / 10u128.pow(24)
}

/// Checks that two amount are within epsilon
pub fn almost_equal(left: Balance, right: Balance, epsilon: Balance) -> bool {
    if left > right {
        (left - right) < epsilon
    } else {
        (right - left) < epsilon
    }
}

#[macro_export]
macro_rules! assert_eq_in_near {
    ($a:expr, $b:expr) => {
        assert_eq!(yton($a), yton($b))
    };
    ($a:expr, $b:expr, $c:expr) => {
        assert_eq!(yton($a), yton($b), $c)
    };
}

#[cfg(test)]
pub mod tests {
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{testing_env, VMContext};

    use crate::*;

    use super::*;

    pub const ONE_EPOCH_TS: u64 = 12 * 60 * 60 * 1_000_000_000;

    pub struct Emulator {
        pub contract: StakingContract,
        pub epoch_height: EpochHeight,
        pub block_index: u64,
        pub block_timestamp: u64,
        pub amount: Balance,
        pub locked_amount: Balance,
        last_total_staked_balance: Balance,
        last_total_stake_shares: Balance,
        pub context: VMContext,
    }

    pub fn zero_fee() -> RewardFeeFraction {
        RewardFeeFraction {
            numerator: 0,
            denominator: 1,
        }
    }

    impl Emulator {
        pub fn new(
            owner: AccountId,
            stake_public_key: PublicKey,
            reward_fee_fraction: RewardFeeFraction,
        ) -> Self {
            let context = VMContextBuilder::new()
                .current_account_id(owner.clone())
                .account_balance(ntoy(30))
                .build();
            testing_env!(context.clone());
            let contract = StakingContract::new(owner, stake_public_key, reward_fee_fraction);
            let last_total_staked_balance = contract.total_staked_balance;
            let last_total_stake_shares = contract.total_stake_shares;
            Emulator {
                contract,
                epoch_height: 0,
                block_timestamp: 0,
                block_index: 0,
                amount: ntoy(30),
                locked_amount: 0,
                last_total_staked_balance,
                last_total_stake_shares,
                context,
            }
        }

        fn verify_stake_price_increase_guarantee(&mut self) {
            let total_staked_balance = self.contract.total_staked_balance;
            let total_stake_shares = self.contract.total_stake_shares;
            assert!(
                U256::from(total_staked_balance) * U256::from(self.last_total_stake_shares)
                    >= U256::from(self.last_total_staked_balance) * U256::from(total_stake_shares),
                "Price increase guarantee was violated."
            );
            self.last_total_staked_balance = total_staked_balance;
            self.last_total_stake_shares = total_stake_shares;
        }

        pub fn update_context(&mut self, predecessor_account_id: AccountId, deposit: Balance) {
            self.verify_stake_price_increase_guarantee();
            self.context = VMContextBuilder::new()
                .current_account_id(staking())
                .predecessor_account_id(predecessor_account_id.clone())
                .signer_account_id(predecessor_account_id)
                .attached_deposit(deposit)
                .account_balance(self.amount)
                .account_locked_balance(self.locked_amount)
                .epoch_height(self.epoch_height)
                .block_index(self.block_index)
                .block_timestamp(self.block_timestamp)
                .build();
            testing_env!(self.context.clone());
            println!(
                "Epoch: {}, Deposit: {}, amount: {}, locked_amount: {}",
                self.epoch_height, deposit, self.amount, self.locked_amount
            );
        }

        pub fn simulate_stake_call(&mut self) {
            let total_stake = self.contract.total_staked_balance;
            // Stake action
            self.amount = self.amount + self.locked_amount - total_stake;
            self.locked_amount = total_stake;
            // Second function call action
            self.update_context(staking(), 0);
        }

        pub fn deposit_and_stake(&mut self, account: AccountId, amount: Balance) {
            self.update_context(account.clone(), amount);
            self.contract.deposit();
            self.amount += amount;
            self.update_context(account, 0);
            self.contract.stake(U128(amount));
            self.simulate_stake_call();
        }

        pub fn skip_epochs(&mut self, num: EpochHeight) {
            self.epoch_height += num;
            self.block_index += num * 12 * 60 * 60;
            self.block_timestamp += num * ONE_EPOCH_TS;
            self.locked_amount = (self.locked_amount * (100 + u128::from(num))) / 100;
            self.update_context(staking(), 0);
        }
    }
}
