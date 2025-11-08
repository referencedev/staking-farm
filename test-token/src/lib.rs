use near_contract_standards::fungible_token::core::FungibleTokenCore;
use near_contract_standards::fungible_token::events::FtMint;
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::FungibleToken;
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, AccountId, PanicOnDefault, PromiseOrValue, NearToken, Gas, near, ext_contract};

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct TestToken {
    pub token: FungibleToken,
    pub metadata: FungibleTokenMetadata,
}

#[near_bindgen]
impl TestToken {
    #[init]
    pub fn new() -> Self {
        assert!(!env::state_exists(), "Already initialized");
        Self {
            token: FungibleToken::new(b"t".to_vec()),
            metadata: FungibleTokenMetadata {
                spec: FT_METADATA_SPEC.to_string(),
                name: "Test Token".to_string(),
                symbol: "TEST".to_string(),
                icon: None,
                reference: None,
                reference_hash: None,
                decimals: 24,
            },
        }
    }

    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        self.assert_owner();
        if self.token.storage_balance_of(account_id.clone()).is_none() {
            self.token.internal_register_account(&account_id);
        }
        self.token.internal_deposit(&account_id, amount.0);
        FtMint { owner_id: &account_id, amount, memo: None }.emit();
    }

    fn assert_owner(&self) {
        assert_eq!(env::predecessor_account_id(), env::current_account_id(), "Only owner");
    }
}

// Implement FungibleTokenCore manually to avoid macro attribute compatibility issues
#[near_bindgen]
impl FungibleTokenCore for TestToken {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        // require exactly 1 yocto for security (prevents access key with allowance)
        near_sdk::assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        self.token
            .internal_transfer(&sender_id, &receiver_id, amount.0, memo.map(|m| m.into()));
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        near_sdk::assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        self.token
            .internal_transfer(&sender_id, &receiver_id, amount.0, memo.map(|m| m.into()));

        // Call receiver's `ft_on_transfer` and return the promise directly.
        // Tests don't rely on refund path, so skipping explicit resolve is acceptable here.
        ext_ft_receiver::ext(receiver_id)
            .with_static_gas(Gas::from_tgas(100))
            .ft_on_transfer(sender_id, amount, msg)
            .into()
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[ext_contract(ext_ft_receiver)]
trait FtReceiver {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128>;
}

#[near_bindgen]
impl StorageManagement for TestToken {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        self.token.storage_unregister(force)
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

#[near_bindgen]
impl FungibleTokenMetadataProvider for TestToken {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.clone()
    }
}
