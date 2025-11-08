use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, AccountId, Gas, PanicOnDefault, PromiseOrValue};

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct MockReceiver {
    /// Mode determines behavior:
    /// "accept_all" - return 0 (use all tokens)
    /// "refund_all" - return full amount (refund everything)
    /// "refund_half" - return half (partial refund)
    /// "burn_gas" - consume excessive gas then return 0
    /// "panic" - panic immediately
    pub mode: String,
}

#[near_bindgen]
impl MockReceiver {
    #[init]
    pub fn new(mode: String) -> Self {
        Self { mode }
    }

    /// Standard NEP-141 receiver callback
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let _sender = sender_id;
        let _msg = msg;

        match self.mode.as_str() {
            "accept_all" => {
                // Accept all tokens (return 0 unused)
                PromiseOrValue::Value(U128(0))
            }
            "refund_all" => {
                // Refund everything
                PromiseOrValue::Value(amount)
            }
            "refund_half" => {
                // Refund half
                PromiseOrValue::Value(U128(amount.0 / 2))
            }
            "burn_gas" => {
                // Burn a lot of gas in a loop, then try to return
                // This simulates running out of gas during callback
                let start_gas = env::used_gas();
                let mut counter = 0u64;
                
                // Burn gas until we've used a significant amount
                // This will cause the callback to fail with "out of gas"
                while env::used_gas().as_gas() < start_gas.as_gas() + Gas::from_tgas(80).as_gas() {
                    counter = counter.wrapping_add(1);
                    // Do some work to actually consume gas
                    if counter % 1000 == 0 {
                        env::log_str(&format!("counter: {}", counter));
                    }
                }
                
                // Try to return 0 (accept all) - but we'll likely run out of gas first
                PromiseOrValue::Value(U128(0))
            }
            "panic" => {
                // Explicit panic
                panic!("MockReceiver explicit panic");
            }
            _ => {
                // Default: accept all
                PromiseOrValue::Value(U128(0))
            }
        }
    }

    /// Helper to change mode
    pub fn set_mode(&mut self, mode: String) {
        self.mode = mode;
    }

    /// Helper to get current mode
    pub fn get_mode(&self) -> String {
        self.mode.clone()
    }
}
