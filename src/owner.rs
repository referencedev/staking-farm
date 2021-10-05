use crate::*;

///*******************/
///* Owner's methods */
///*******************/
#[near_bindgen]
impl StakingContract {
    /// Storing owner in a separate storage to avoid STATE corruption issues.
    /// Returns previous owner if it existed.
    pub(crate) fn internal_set_owner(&self, owner_id: &AccountId) -> Option<AccountId> {
        env::storage_write(b"OWNER", owner_id.as_bytes());
        env::storage_get_evicted()
            .map(|bytes| AccountId::new_unchecked(String::from_utf8(bytes).expect("INTERNAL FAIL")))
    }

    pub fn set_owner_id(&self, owner_id: &AccountId) {
        let prev_owner = self.internal_set_owner(owner_id).expect("MUST HAVE OWNER");
        assert_eq!(
            prev_owner,
            env::predecessor_account_id(),
            "MUST BE OWNER TO SET OWNER"
        );
    }

    /// Returns current owner from the storage.
    pub fn get_owner_id(&self) -> AccountId {
        AccountId::new_unchecked(
            String::from_utf8(env::storage_read(b"OWNER").expect("MUST HAVE OWNER"))
                .expect("INTERNAL_ FAIL"),
        )
    }

    /// Owner's method.
    /// Updates current public key to the new given public key.
    pub fn update_staking_key(&mut self, stake_public_key: PublicKey) {
        self.assert_owner();
        // When updating the staking key, the contract has to restake.
        let _need_to_restake = self.internal_ping();
        self.stake_public_key = stake_public_key.into();
        self.internal_restake();
    }

    /// Owner's method.
    /// Updates current reward fee fraction to the new given fraction.
    pub fn update_reward_fee_fraction(&mut self, reward_fee_fraction: RewardFeeFraction) {
        self.assert_owner();
        reward_fee_fraction.assert_valid();

        let need_to_restake = self.internal_ping();
        self.reward_fee_fraction = reward_fee_fraction;
        if need_to_restake {
            self.internal_restake();
        }
    }

    /// Owner's method.
    /// Pauses pool staking.
    pub fn pause_staking(&mut self) {
        self.assert_owner();
        assert!(!self.paused, "The staking is already paused");

        self.internal_ping();
        self.paused = true;
        Promise::new(env::current_account_id()).stake(0, self.stake_public_key.clone());
    }

    /// Owner's method.
    /// Resumes pool staking.
    pub fn resume_staking(&mut self) {
        self.assert_owner();
        assert!(self.paused, "The staking is not paused");

        self.internal_ping();
        self.paused = false;
        self.internal_restake();
    }
}
