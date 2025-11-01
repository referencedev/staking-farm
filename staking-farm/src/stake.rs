use crate::internal::{MIN_BURN_AMOUNT, ZERO_ADDRESS};
use crate::*;

/// Interface for the contract itself.
#[ext_contract(ext_self)]
#[allow(dead_code)]
pub trait SelfContract {
    /// A callback to check the result of the staking action.
    /// In case the stake amount is less than the minimum staking threshold, the staking action
    /// fails, and the stake amount is not changed. This might lead to inconsistent state and the
    /// follow withdraw calls might fail. To mitigate this, the contract will issue a new unstaking
    /// action in case of the failure of the first staking action.
    fn on_stake_action(&mut self);

    /// Check if reward withdrawal succeeded and if it failed, refund reward back to the user.
    fn callback_post_withdraw_reward(
        &mut self,
        token_id: AccountId,
        sender_id: AccountId,
        amount: U128,
    );

    /// Callback after getting the owner of the given account.
    fn callback_post_get_owner(
        &mut self,
        token_id: AccountId,
        delegator_id: AccountId,
        account_id: AccountId,
    ) -> Promise;

    /// Resolve FT transfer for stake shares and refund unused amount back to sender.
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128;
}

#[near]
impl StakingContract {
    /// Deposits the attached amount into the inner account of the predecessor.
    #[payable]
    pub fn deposit(&mut self) {
        let need_to_restake = self.internal_ping();

        self.internal_deposit();

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Deposits the attached amount into the inner account of the predecessor and stakes it.
    #[payable]
    pub fn deposit_and_stake(&mut self) {
        self.internal_ping();

        let amount = self.internal_deposit();
        self.internal_stake(amount);

        self.internal_restake();
    }

    /// Withdraws the entire unstaked balance from the predecessor account.
    /// It's only allowed if the `unstake` action was not performed in the four most recent epochs.
    pub fn withdraw_all(&mut self) {
        let need_to_restake = self.internal_ping();

        let account_id = env::predecessor_account_id();
        let account = self.internal_get_account(&account_id);
        self.internal_withdraw(&account_id, account.unstaked);

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Withdraws the non staked balance for given account.
    /// It's only allowed if the `unstake` action was not performed in the four most recent epochs.
    pub fn withdraw(&mut self, amount: U128) {
        let need_to_restake = self.internal_ping();

        let amount: Balance = amount.into();
        self.internal_withdraw(&env::predecessor_account_id(), amount);

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Stakes all available unstaked balance from the inner account of the predecessor.
    pub fn stake_all(&mut self) {
        // Stake action always restakes
        self.internal_ping();

        let account_id = env::predecessor_account_id();
        let account = self.internal_get_account(&account_id);
        self.internal_stake(account.unstaked);

        self.internal_restake();
    }

    /// Stakes the given amount from the inner account of the predecessor.
    /// The inner account should have enough unstaked balance.
    pub fn stake(&mut self, amount: U128) {
        // Stake action always restakes
        self.internal_ping();

        let amount: Balance = amount.into();
        self.internal_stake(amount);

        self.internal_restake();
    }

    /// Unstakes all staked balance from the inner account of the predecessor.
    /// The new total unstaked balance will be available for withdrawal in four epochs.
    pub fn unstake_all(&mut self) {
        self.internal_unstake_all(&env::predecessor_account_id());
    }

    /// Unstakes the given amount from the inner account of the predecessor.
    /// The inner account should have enough staked balance.
    /// The new total unstaked balance will be available for withdrawal in four epochs.
    pub fn unstake(&mut self, amount: U128) {
        // Unstake action always restakes
        self.internal_ping();

        let amount: Balance = amount.into();
        self.inner_unstake(&env::predecessor_account_id(), amount);

        self.internal_restake();
    }

    /// Unstakes all the tokens that must be burnt.
    pub fn unstake_burn(&mut self) {
        self.internal_unstake_all(&ZERO_ADDRESS.parse().expect("INTERNAL FAIL"));
    }

    /// Burns all the tokens that are unstaked.
    pub fn burn(&mut self) {
        let account_id: AccountId = ZERO_ADDRESS.parse().expect("INTERNAL FAIL");
        let account = self.internal_get_account(&account_id);
        if account.unstaked > MIN_BURN_AMOUNT {
            // TODO: replace with burn host function when available.
            self.internal_withdraw(&account_id, account.unstaked);
        }
    }

    /*************/
    /* Callbacks */
    /*************/

    pub fn on_stake_action(&mut self) {
        assert_eq!(
            env::current_account_id(),
            env::predecessor_account_id(),
            "Can be called only as a callback"
        );

        assert_eq!(
            env::promise_results_count(),
            1,
            "Contract expected a result on the callback"
        );
        let stake_action_succeeded = matches!(env::promise_result(0), PromiseResult::Successful(_));

        // If the stake action failed and the current locked amount is positive, then the contract
        // has to unstake.
        if !stake_action_succeeded && env::account_locked_balance() > NearToken::from_yoctonear(0) {
            Promise::new(env::current_account_id())
                .stake(NearToken::from_yoctonear(0), self.stake_public_key.clone());
        }
    }

    /// Internal transfer of stake shares between two normal accounts.
    /// Distributes rewards for both sides before moving shares.
    pub(crate) fn internal_share_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        amount: Balance,
    ) {
        assert!(amount > 0, "ERR_ZERO_AMOUNT");
        assert!(sender_id != receiver_id, "ERR_SAME_ACCOUNT");
        // Update epoch/rewards; no need to restake here.
        self.internal_ping();

        // Sender must have enough shares.
        let mut sender = self.internal_get_account(sender_id);
        let mut receiver = self.internal_get_account(receiver_id);

        // Distribute rewards so future farm calculations use updated stake_shares.
        self.internal_distribute_all_rewards(&mut sender);
        self.internal_distribute_all_rewards(&mut receiver);

        assert!(sender.stake_shares >= amount, "ERR_INSUFFICIENT_SHARES");
        sender.stake_shares -= amount;
        receiver.stake_shares += amount;

        self.internal_save_account(sender_id, &sender);
        self.internal_save_account(receiver_id, &receiver);
    }
}

#[near]
impl StakingContract {
    /// Resolve FT transfer per NEP-141. Expects that receiver's ft_on_transfer returned amount to refund.
    #[private]
    pub fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        use near_sdk::PromiseResult;
        let amount: Balance = amount.0;
        let unused = match env::promise_result(0) {
            PromiseResult::Successful(value) => near_sdk::serde_json::from_slice::<U128>(&value)
                .map(|v| v.0)
                .unwrap_or(0),
            _ => 0,
        };
        if unused > 0 {
            // Refund unused shares from receiver back to sender; clamp to originally sent amount and receiver balance.
            let receiver = self.internal_get_account(&receiver_id);
            let refund = std::cmp::min(unused, std::cmp::min(amount, receiver.stake_shares));
            if refund > 0 {
                // Perform transfer back without callbacks.
                self.internal_share_transfer(&receiver_id, &sender_id, refund);
                return U128(refund);
            }
        }
        U128(0)
    }
}
