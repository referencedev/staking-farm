# Stake & earn

Staking farm contract allows for validators to distribute other tokens to the delegators.

This allows to attract more capital to the validator while ensuring more robust token distribution of the new tokens.

## Authorized users

Because of storage and computational limitations, the contract can only store a fixed number of farms.
To avoid farm spam, only authorized users can deposit tokens. 
Owner of the contract can manage authorized users or can deposit farms itself.

## Create new farm

Use `ft_transfer_call` of the token to the staking farm by an authorized user to create new farm.

Farm contains next fields:
 - name: String,
 - token_id: AccountId,
 - amount: Balance,
 - start_date: Timestamp,
 - end_date: Timestamp,
 - last_distribution: RewardDistribution,

## Upgradability

Staking Farm contract supports upgradability from the specific factory contract.
This is done to ensure that both contract owner and general community agree on the contract upgrade before it happens.

The procedure for upgrades is as follows:
 - Staking contract has the `factory_id` specified. This `factory_id` should be governed by the users or Foundation that users trust. 
 - Factory contract contains whitelisted set of contracts, addressed by hash.
 - Contract owner calls `upgrade(contract_hash)` method, which loads contract bytecode from `factory_id` and upgrade itself in-place.

To avoid potential issues with state serialization failures due to upgrades, the owner information is stored outside of the STATE storage.
This ensures that if new contracts has similar `upgrade` method that doesn't use state, even if contract got into wrong state after upgrade it is resolvable.

## Burning rewards

The staking reward contract has a feature to burn part of the rewards.
NEAR doesn't have currently a integrated burning logic, so instead a `ZERO_ADDRESS` is used. This is an implicit address of `0` and that doesn't have any access keys: https://explorer.mainnet.near.org/accounts/0000000000000000000000000000000000000000000000000000000000000000

The burning itself is done in a 3 steps:
 - When epoch ends and `ping` is called, the amount of rewawrds allocated to burn will be transferred to `ZERO_ADDRESS` address via shares. This shares are still staked.
 - Anyone can call `unstake_burn`, which will unstake all the currently staked shares on `ZERO_ADDRESS`.
 - After 36 hours of unstaking, anyone can call `burn` to actually transfer funds to `ZERO_ADDRESS`.

This is done because transferring immediately rewards to `ZERO_ADDRESS` is impossible as they are already staked when allocated.
Anyone can call `unstake_burn` and `burn`, similarly how anyone can call `ping` on the staking pool to kick the calculations.

TODO: the imporvement to this method, would be to unstake that amount direclty on `ping` and just let it be burnt via the subsequent `burn` call.

## Stake shares as Fungible Token (NEP-141)

This contract exposes staked shares as a standard NEAR FT (NEP-141), so users can transfer their staking position to other accounts.

- Token metadata (NEP-148):
		- name: defaults to the full account ID of this contract (e.g. `staking.pool.near`)
		- symbol: defaults to the prefix of the contract account ID before the first dot (e.g. `staking`)
		- decimals: 24

	The owner can update these values at any time without a state migration, since they’re stored outside of contract STATE:

	- set name: `set_ft_name(name: String)`
	- set symbol: `set_ft_symbol(symbol: String)`

	Notes:
	- Changing metadata doesn’t affect token balances or supply.
	- Defaults are used unless explicitly overwritten by the owner.

### Storage (NEP-145)

Explicit storage registration is required when transferring to the new user who hasn't staked with given contract. 

The storage interface is provided for compatibility with wallets/dapps:

- `storage_balance_bounds` returns amount required to cover Account storage
- `storage_balance_of` returns either amount for registered storage or 0
- `storage_deposit` records storage payment but doesn't create Account
- `storage_withdraw` refunds storage if Account doesn't exist anymore

### Transfer shares

Transfer a specific number of shares (requires 1 yoctoNEAR):

```bash
near call <pool_id> ft_transfer '{"receiver_id": "<receiver_id>", "amount": "<yocto_shares>"}' --accountId <sender_id> --amount 0.000000000000000000000001
```

Use `ft_transfer_call` to send shares to a contract that implements `ft_on_transfer`:

```bash
near call <pool_id> ft_transfer_call '{"receiver_id": "<contract_id>", "amount": "<yocto_shares>", "msg": "<json>"}' --accountId <sender_id> --amount 0.000000000000000000000001 --gas 30000000000000
```

The receiver can return a numeric string for the number of shares it wants to refund. Any unused shares will be returned to the sender via `ft_resolve_transfer`.

### Supply and balances

- `ft_total_supply` equals all minted stake shares minus burned shares held by the implicit burn account.
- `ft_balance_of(account_id)` returns the account’s current stake share balance.
- Transfers to the burn account are blocked; burning continues to work via the existing burn flow.

### Notes

- Moving shares moves the right to future staking rewards and farm distributions. Before moving shares, the contract distributes pending farm rewards to both sender and receiver at their current share balances to keep accounting correct.
- You still use the staking methods (`deposit`, `stake`, `unstake`, `withdraw`) for NEAR; FT only represents the transferable share units.
