use crate::internal::{MIN_BURN_AMOUNT, ZERO_ADDRESS};
use crate::*;

/// Interface for the contract itself.
#[ext_contract(ext_self)]
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
}

#[near_bindgen]
impl StakingContract {
    /// Deposits the attached amount into the inner account of the predecessor.
    #[payable]
    pub fn deposit(&mut self) {
        let need_to_restake = self.internal_ping();

        self.internal_deposit(true);

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Deposits the attached amount into the inner account of the precedessor, but the inner account
    /// is attached to the staking pool that doesnt restake rewards
    #[payable]
    pub fn deposit_rewards_not_stake(&mut self){
        let need_to_restake = self.internal_ping();

        self.internal_deposit(false);

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Deposits the attached amount into the inner account of the predecessor and stakes it.
    #[payable]
    pub fn deposit_and_stake(&mut self) {
        self.internal_ping();

        let amount = self.internal_deposit(true);
        self.internal_stake(amount);

        self.internal_restake();
    }

    /// Deposits the attached amount into the inner account of the predecessor and stakes it to the inner pool
    /// that doesnt restake rewards
    #[payable]
    pub fn deposit_and_stake_rewards_not_stake(&mut self){
        self.internal_ping();

        let amount = self.internal_deposit(false);
        self.internal_stake(amount);

        self.internal_restake();
    }

    /// Withdraws the entire unstaked balance from the predecessor account.
    /// It's only allowed if the `unstake` action was not performed in the four most recent epochs.
    pub fn withdraw_all(&mut self, receiver_account_id: AccountId) {
        let need_to_restake = self.internal_ping();

        let account_id = env::predecessor_account_id();
        let account_unstaked = self.get_account_unstaked_balance(account_id.clone()).0;
        self.internal_withdraw(&account_id, receiver_account_id , account_unstaked, true);

        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Withdraws the non staked balance for given account.
    /// It's only allowed if the `unstake` action was not performed in the four most recent epochs.
    pub fn withdraw(&mut self, amount: U128, receiver_account_id: AccountId) {
        let need_to_restake = self.internal_ping();

        let amount: Balance = amount.into();
        self.internal_withdraw(&env::predecessor_account_id(), receiver_account_id, amount, false);

        if need_to_restake {
            self.internal_restake();
        }
    }

     /// Withdraw rewards that are being collected for accounts that doesnt restake their rewards
     pub fn withdraw_rewards(&mut self, receiver_account_id: AccountId){
        let need_to_restake = self.internal_ping();
        
        self.internal_withdraw_rewards(&receiver_account_id);
        if need_to_restake{
            self.internal_restake();
        }
    }

    /// Stakes all available unstaked balance from the inner account of the predecessor.
    pub fn stake_all(&mut self) {
        // Stake action always restakes
        self.internal_ping();

        let account_id = env::predecessor_account_id();
        let unstaked_balance = self.get_account_unstaked_balance(account_id.clone());
        self.internal_stake(unstaked_balance.0);

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
        self.internal_unstake_all(&AccountId::new_unchecked(ZERO_ADDRESS.to_string()));
    }

    /// Burns all the tokens that are unstaked.
    pub fn burn(&mut self) {
        let account_id = AccountId::new_unchecked(ZERO_ADDRESS.to_string());
        let account = self.rewards_staked_staking_pool.internal_get_account(&account_id);
        if account.unstaked > MIN_BURN_AMOUNT {
            // TODO: replace with burn host function when available.
            self.internal_withdraw(&account_id, account_id.clone(), account.unstaked, false);
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
        let stake_action_succeeded = match env::promise_result(0) {
            PromiseResult::Successful(_) => true,
            _ => false,
        };

        // If the stake action failed and the current locked amount is positive, then the contract
        // has to unstake.
        if !stake_action_succeeded && env::account_locked_balance() > 0 {
            Promise::new(env::current_account_id()).stake(0, self.stake_public_key.clone());
        }
    }
}
