use near_workspaces::types::{Gas, NearToken};
use near_workspaces::{sandbox, Account, AccountId, Contract, Worker};
use serde_json::json;

const STAKING_POOL_ACCOUNT_ID: &str = "pool";
const STAKING_KEY: &str = "KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7";
const STAKING_FARM_WASM: &str = "../target/near/staking_farm/staking_farm.wasm";

struct Ctx {
    worker: Worker<near_workspaces::network::Sandbox>,
    owner: Account,
    pool: Contract,
}

async fn init_pool(pool_initial_balance: NearToken, reward_ratio: u32, burn_ratio: u32) -> anyhow::Result<Ctx> {
    let worker = sandbox().await?;
    let owner = worker.root_account()?;

    // Deploy staking pool
    let pool_wasm = std::fs::read(STAKING_FARM_WASM)?;
    let pool = owner
        .create_subaccount(STAKING_POOL_ACCOUNT_ID)
        .initial_balance(pool_initial_balance)
        .transact()
        .await?
        .into_result()?;
    let pool = pool.deploy(&pool_wasm).await?.into_result()?;

    let reward_ratio = json!({"numerator": reward_ratio, "denominator": 10});
    let burn_ratio = json!({"numerator": burn_ratio, "denominator": 10});
    pool
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "stake_public_key": STAKING_KEY,
            "reward_fee_fraction": reward_ratio,
            "burn_fee_fraction": burn_ratio
        }))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    Ok(Ctx { worker, owner, pool })
}

async fn create_user_and_stake(ctx: &Ctx, name: &str, stake_amount: NearToken) -> anyhow::Result<Account> {
    let user = ctx
        .owner
        .create_subaccount(name)
        .initial_balance(NearToken::from_near(100_000))
        .transact()
        .await?
        .into_result()?;

    user
        .call(ctx.pool.id(), "deposit_and_stake")
        .deposit(stake_amount)
        .gas(Gas::from_tgas(200))
        .transact()
        .await?
        .into_result()?;

    Ok(user)
}

/// Test ft_transfer_call where the receiver contract runs out of gas in the callback, ensuring resolver logic is triggered.
#[tokio::test]
async fn test_ft_transfer_call_receiver_out_of_gas() -> anyhow::Result<()> {
    let ctx = init_pool(NearToken::from_near(10_000), 0, 0).await?;

    // Create user and stake
    let user1 = create_user_and_stake(&ctx, "user1", NearToken::from_near(5_000)).await?;

    // Deploy the mock receiver contract (burns all gas in ft_on_transfer)
    let mock_receiver_wasm = std::fs::read("../target/near/mock_receiver/mock_receiver.wasm")?;
    let mock_receiver = ctx
        .owner
        .create_subaccount("mockreceiver")
        .initial_balance(NearToken::from_near(10))
        .transact()
        .await?
        .into_result()?;
    let mock_receiver = mock_receiver.deploy(&mock_receiver_wasm).await?.into_result()?;
    // Initialize and set mode to burn_gas
    mock_receiver
        .call("new")
        .args_json(json!({"mode": "burn_gas"}))
        .transact()
        .await?
        .into_result()?;

    // Register mock receiver for FT shares (no-op but standard API)
    let _ = ctx
        .owner
        .call(ctx.pool.id(), "storage_deposit")
        .args_json(json!({ "account_id": mock_receiver.id(), "registration_only": true }))
        .deposit(NearToken::from_millinear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?;

    // Query user1 shares
    let user1_shares: u128 = ctx
        .pool
        .view("ft_balance_of")
        .args_json(json!({ "account_id": user1.id() }))
        .await?
        .json::<serde_json::Value>()?
        .as_str()
        .unwrap()
        .parse()?;
    let transfer_amount = user1_shares / 5;

    // Perform ft_transfer_call with enough gas for transfer, but not enough for callback to succeed
    let result = user1
        .call(ctx.pool.id(), "ft_transfer_call")
        .args_json(json!({
            "receiver_id": mock_receiver.id(),
            "amount": transfer_amount.to_string(),
            "msg": "test burn gas"
        }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await;

    // The transaction should succeed, but the callback will run out of gas; resolver should treat as used (no refund)
    match result {
        Ok(outcome) => {
            let _ = outcome.into_result()?;
        }
        Err(e) => panic!("Transaction should succeed, got error: {:?}", e),
    }

    // Validate balances
    let user1_after: u128 = ctx
        .pool
        .view("ft_balance_of")
        .args_json(json!({ "account_id": user1.id() }))
        .await?
        .json::<serde_json::Value>()?
        .as_str()
        .unwrap()
        .parse()?;
    let receiver_after: u128 = ctx
        .pool
        .view("ft_balance_of")
        .args_json(json!({ "account_id": mock_receiver.id() }))
        .await?
        .json::<serde_json::Value>()?
        .as_str()
        .unwrap()
        .parse()?;
    assert_eq!(user1_after, user1_shares - transfer_amount, "User1 shares should decrease");
    assert_eq!(receiver_after, transfer_amount, "Receiver should get all transferred shares");
    Ok(())
}
