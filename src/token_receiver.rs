use near_sdk::json_types::{U128, U64};
use near_sdk::{PromiseOrValue, serde_json, env};

use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct FarmingDetails {
    start_date: U64,
    end_date: U64,
}

#[near_bindgen]
impl FungibleTokenReceiver for StakingContract {
    /// Callback on receiving tokens by this contract.
    /// transfer reward token with specific msg indicate
    /// which farm to be deposited to.
    #[allow(unused_variables)]
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let message =
            serde_json::from_str::<FarmingDetails>(&msg).expect("ERR_MSG_WRONG_FORMAT");
        self.internal_deposit_farm_tokens(&env::predecessor_account_id(), amount.0, message.start_date.0, message.end_date.0);
        PromiseOrValue::Value(U128(0))
    }
}
