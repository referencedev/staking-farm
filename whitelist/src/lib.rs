use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::store::IterableSet;
use near_sdk::{env, near_bindgen, AccountId, PanicOnDefault};

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Whitelist {
    owner: AccountId,
    pools: IterableSet<AccountId>,
}

#[near_bindgen]
impl Whitelist {
    #[init]
    pub fn new(foundation_account_id: AccountId) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        Self {
            owner: foundation_account_id,
            pools: IterableSet::new(b"p".to_vec()),
        }
    }

    pub fn add_staking_pool(&mut self, staking_pool_account_id: AccountId) {
        // Only owner can add
        assert_eq!(env::predecessor_account_id(), self.owner, "Not owner");
    self.pools.insert(staking_pool_account_id);
    }

    pub fn is_whitelisted(&self, account_id: AccountId) -> bool {
        self.pools.contains(&account_id)
    }
}
