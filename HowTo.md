# Near: Staking and Farming Howto.

This document describes how to deploy, configure and use the new Staking-Pool with Farming contract.

## How does Stake & Farm Work?

Staking is the process of delegating NEAR to a staking pool that is operated by a Near validator node.
Stake-Holders participate in the rewards earned by the validator node.

Farming allows stake-holders to temporarily lock NEAR with a farming contract. During
this time of locking the stake-holder receives a faction of tokens that are distributed
by the farming contract.

The new Stake & Farm contract allows both at the same time: Stake NEAR with the contract,
and earn validator rewards AND farm tokens at the same time.

## Deploying the complete contract environment for Stake&Farm on the NEAR testnet.

This section is of interest for developers and testers. 

If you are only interested in deploying a Stake&Farm Validator, jump ahead to "Deploy Stake&Farm".

If you are only interested in staking with an existing Stake&Farm Validator, jump ahead to "Stake Near and Farm Tokens".

### Required tools

You need:

  1. GIT: https://git-scm.com/
  2. NEAR CLI: https://github.com/near/near-cli#Installation
  3. Go: https://go.dev/doc/install
  4. Install the nearkey tool:\
  `$ go install github.com/aurora-is-near/near-api-go/tools/cmd/nearkey`\
  6. Install the nearcall tool (only if you are deploying the whitelist and factory contracts):\
  `$ go install github.com/aurora-is-near/near-api-go/tools/cmd/nearcall`\
  This tool allows you to call contract methods with arguments that are too long for the near cli.
  7. Rust (only if you are deploying the whitelist and factory contracts):\
  `$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`\
  `$ rustup target add wasm32-unknown-unknown`

### Get a testnet account

You will require a testnet account with enough testnet NEAR to follow these instructions.

Create it by calling:\
  `$ near login`

Then follow the instructions in the browser window to create a new account and grant permissions for near cli to it.

Make the account name globally available:\
  `$ export MASTERACC=master.testnet`

Replace *master.testnet* with the actual name of the account you just created.

### Deploy the whitelist contract

Whitelisting controls which contracts can accept locked NEAR for staking/farming. The official whitelisting
contracts are operated by the NEAR Foundation. We're setting up our own whitelisting contract here to create a complete
testing environment.

 1. Clone the Near Core Contracts repository which contains the whitelist contract:\
 `$ git clone https://github.com/near/core-contracts.git`
 2. The whitelist contract can be found in the directory core-contracts/whitelist:\
 `$ cd core-contracts/whitelist`
 3. Build the contract (this will take a moment):\
 `$ ./build.sh`
 4. The compiled contract can be found in the directory res now:\
 `$ cd res`
 5. Create the contract account for the whitelist contract:\
 `$ near --masterAccount ${MASTERACC} create-account whitelist.${MASTERACC}`
 6. Deploy the whitelist contract:\
 `$ near --masterAccount ${MASTERACC} deploy --accountId whitelist.${MASTERACC}
  --wasmFile whitelist.wasm --initFunction new --initArgs '{"foundation_account_id":"'${MASTERACC}'"}'  --initGas 300000000000000 `\
 This deploys the whitelist contract under the name *whitelist.${MASTERACC}* and configures the controlling account
 to *${MASTERACC}*. 
 
### Deploy the Factory contract

A factory contract is used to create many instances of the same contract on NEAR. This is especially important
for staking contracts that want to be able to receive stakes from locked NEAR. Only a factory can
provide the deploy and update functions while at the same time giving some assurance that the contract is meeting
security requirement of the NEAR foundation. Furthermore it greatly simplifies deployment.

  1. Clone the Stake&Farm repository that contains the factory contract:\
  `$ git clone https://github.com/referencedev/staking-farm.git`
  2. The factory contract is located in the directory staking-farm/staking-factory:\
  `$ cd staking-farm/staking-factory`
  3. Build the factory contract:\
  `$ ./build_local.sh`
  4. The compiled contract is located in ../res/staking_factory_local.wasm.\
  `$ cd ../res/`
  5. Create the contract account for the factory contract:\
  `$ near --masterAccount ${MASTERACC} create-account factory.${MASTERACC}`
  6. Deploy the factory contract:\
  `$ near --masterAccount ${MASTERACC} deploy --accountId factory.${MASTERACC} 
  --wasmFile staking_factory.wasm --initFunction new 
  --initArgs '{"owner_id":"'${MASTERACC}'", "staking_pool_whitelist_account_id":"whitelist.'${MASTERACC}'"}'
  --initGas 300000000000000 `\
  This creates the contract factory that is controlled by ${MASTERACC} and is referring the whitelisting contract deployed before,\
  whitelist.${MASTERACC}.

### Whitelist the contract factory

The newly deployed factory needs the blessing of the whitelisting contract:

`$ near --accountId ${MASTERACC} call whitelist.${MASTERACC} add_factory '{"factory_account_id":"factory.'${MASTERACC}'"}'`

You can verify the success by calling:

`$ near --accountId ${MASTERACC} view whitelist.${MASTERACC} is_factory_whitelisted '{"factory_account_id":"factory'${MASTERACC}'"}'`

The result of that call should be "true".

### Load stake&farm contract into the contract factory.

The contract factory needs actual contracts that it can deploy:

  1. In the cloned Stake&Farm repository navigate to the staking-farm directory:\
  `$ cd ../staking-farm`
  2. Build the contract:\
  `$ ./build_local.sh`
  3. The compiled contract is located in ../res/staking_factory_local.wasm.\
  `$ cd ../res/`
  4. Deploying the contract into the factory requires the nearcall tool installed before:\
  `$ near call -account ${MASTERACC} -contract factory.${MASTERACC} -args staking_farm_local.wasm`\
  This will return an "Argument hash" that is required for later, and success/failure information of the deployment call.\
  Make the argument hash globally available:\
  `$ export CONTRACTHASH=HxT6MrNC7...`
  5. Whitelist the contract hash:\
  `$ near call --accountId ${MASTERACC} call factory.${MASTERACC} allow_contract '{"code_hash": "'${CONTRACTHASH}'"}'`
  
You can verify the previous call by:

`$ near --accountId ${MASTERACC} call factory.${MASTERACC} get_code '{"code_hash":"'${CONTRACTHASH}'"}'`

The result of this call should be a lot of "garbage": That's the loaded contract code.

The prerequisites for actually deploying a Stake&Farm contract are now in place.

## Deploy Stake&Farm

  1. First, make some configuration globally available:\
  `$ export CONTRACTHASH=HxT6MrNC7cQh68CZeBxiBbePSD7rxDeqeQDeHQ8n5j2M`\
  `$ export FACTORY=factory01.littlefarm.testnet`\
  `$ export WHITELIST=whitelist01.littlefarm.testnet`\
  The above refer to an example factory installation on testnet. If you deployed a factory yourself, use the data from the above steps.\
  For production deployment, NEAR will provide official values both for testnet and mainnet.
  2. Create a controlling account for your stake&farm contract:\
  `$ near login`\
  Then follow the instructions in the browser window to create a new account and grant permissions for near cli to it.
  3. Make the account name globally available:\
  `$ export OWNERACC=owner.testnet`\
  Replace *owner.testnet* with the actual name of the account you just created.
  4. Select a name for your validator and make it globally available:\
  `$ export VALIDATORNAME=myvalidator`
  5. Create the validator keypair:\
  `$ nearkey ${VALIDATORNAME}.${FACTORY} > validator_key.json`\
  This creates a file "validator_key.json" that needs to be deployed to your nearcore node installation.
  6. Copy the public_key from the validator_key.json file and make it publicly available:\
  `$ export VALIDATORKEY="ed25519:eSNAthKiUM1kNFifPDCt6U83Abnak4dCRbhUeNGA9j7"`
  7. Finally, call the factory to create the new stake&farm contract:\
  `$ near --accountId ${OWNERACC} call ${FACTORY} create_staking_pool '{ "staking_pool_id":"'${VALIDATORNAME}'", "code_hash":"'${CONTRACTHASH}'",  "stake_public_key":"'${VALIDATORKEY}'", "reward_fee_fraction": {"numerator": 10, "denominator": 100}}' --amount 30 --gas 300000000000000`\
  This deploys the staking contract owned by OWNERACC and keeping 10/100 (numerator/denominator) of rewards for itself while distributing the remainder to stake-holders.
  8. Make the name of the new contract globally available:\
  `$ export STAKINGCONTRACT=${VALIDATORNAME}.${FACTORY}`
  9. Verify deployment and whitelisting:\
  `$ near view ${WHITELIST} is_whitelisted '{"staking_pool_account_id":"'${STAKINGCONTRACT}''"}'`\
  The result should be "true".
  
### Set up a farm from an example contract

Farming requires a source of tokens to be farmed. The source can be any contract that implements the NEAR "Fungible Tokens" Standard (ft-1.0.0).

Let's create such a contract and use it for farming in our stake&farm contract:

  1. Clone the Rust near-sdk which contains example contracts:\
  `$ git clone https://github.com/near/near-sdk-rs.git`
  2. The example contract is located in near-sdk-rs/examples/fungible-token:\
  `$ cd near-sdk-rs/examples/fungible-token`
  3. Build the contract:\
  `$ ./build.sh`\
  The compiled contract is located in res/fungible_token.wasm.
  4. Create the contract account for the token contract:\
  `$ near --masterAccount ${OWNERACC} create-account token.${OWNERACC}`
  5. Deploy the token contract:\
  `$ near --masterAccount ${OWNERACC} deploy --accountId token.${OWNERACC}
  --wasmFile  res/fungible_token.wasm --initGas 300000000000000` \
  Be aware that the owner of the token contract does not have to be the owner of the stake&farm contract.
  6. Configure the token contract:\
  `$ near --accountId ${OWNERACC} call token.${OWNERACC} new '{"owner_id": "'${OWNERACC}'", "total_supply": "1000000000000000", "metadata": { "spec": "ft-1.0.0", "name": "Example Token", "symbol": "EXT", "decimals": 8 }}'`\
  This creates a token with total supply, name, symbol, etc.

Now that we have tokens to farm, we need to transfer some of them to the stake&farm contract:

  0. The specific token for the farm needs to be whitelisted: `near call ${STAKINGCONTRACT} add_authorized_farm_token '{"token_id": "token.${OWNERACC}"} --accountId ${OWNERACC}'`
  1. The stake&farm contract needs to be able to hold tokens in the token contract. This requires paying the token contract to create storage:\
  `$ near call token.${OWNERACC} storage_deposit '{"account_id": "'${STAKINGCONTRACT}'"}' --accountId ${OWNERACC} --amount 0.00125`
  2. Calculate the time range for the farm:\
  `$ export STARTIN=360` Start farm in 360 seconds.\
  `$ export DURATION=3600` Run the farm for 3600 seconds.\
  `$ export STARTDATE=$(expr $(date +%s) + ${STARTIN})` Calculate the unix timestamp at which to start farming.\
  `$ export ENDDATE=$(expr ${STARTDATE} + ${DURATION})"000000000"` Calculate the unix nanosecond timestamp at which to end farming.\
  `$ export STARTDATE=${STARTDATE}"000000000"` Make startdate nanoseconds.
  3. Create the farm in the stake&farm contract by transferring tokens to it:\
  `$ near call token.${OWNERACC} ft_transfer_call '{"receiver_id": "'${STAKINGCONTRACT}'", "amount": "10000000000000", "msg": "{\"name\": \"Example Token\", \"start_date\": \"'${STARTDATE}'\", \"end_date\": \"'${ENDDATE}'\" }"}' --accountId ${OWNERACC} --amount 0.000000000000000000000001 --gas 300000000000000`\
  The farm is now ready. Stake holders that have a stake in the contract between STARTDATE and ENDDATE will receive a share of the farmed tokens.
  4. Verify that the farm is available:\
  `$ near view ${STAKINGCONTRACT} get_farms '{ "from_index": 0, "limit": 128 }'`\
  This will list the first 128 farms configured in the contract. Right now, only one farm should be returned.
  
## Stake Near and Farm Tokens

Staking NEAR with a stake&farm contract is easy:

  1. Make some settings globally available:\
  `$ export STAKINGCONTRACT=validator2.factory01.littlefarm.testnet` The contract with which you want to stake.\
  `$ export WHITELIST=whitelist01.littlefarm.testnet` The global whitelisting contract.\
  `$ export MYACCOUNT=investment.testnet` The name of your actual NEAR account with which to stake.
  2. Make sure your stake&farm contract is whitelisted:\
  `$ near view ${WHITELIST} is_whitelisted '{"staking_pool_account_id":"'${STAKINGCONTRACT}''"}'`\
  The result should be "true".
  3. Stake some NEAR with the contract:\
  `$ near call ${STAKINGCONTRACT} deposit_and_stake '' --accountId ${MYACCOUNT} --amount 30 --gas 300000000000000`\
  This will stake 30 NEAR with the contract.

### Cashing out NEAR rewards

  1. Unstake all NEAR (you also have the option to only unstake a fraction):\ 
  `$ near call ${STAKINGCONTRACT} unstake_all --accountId ${MYACCOUNT} --gas=300000000000000`\
  This will unstake all your stake in the contract, but the NEAR will not be available for withdrawal for 2-3 epochs.
  2. Withdraw the NEAR:\
  `$ near call ${STAKINGCONTRACT} withdraw_all --accountId ${MYACCOUNT} --gas=300000000000000`\
  This will make your share of the rewards and your initial staking amount available in your wallet again.

For more information, check [staking&delegation](https://docs.near.org/docs/develop/node/validator/staking-and-delegation).


### Cashing out farmed tokens

  1. Check which farms are available in the contract:\
  `$ near view ${STAKINGCONTRACT} get_farms '{ "from_index": 0, "limit": 128 }'`
  2. For each farm, you can calculate how many tokens you earned. The following calls refer to "farm_id", which is the position of the farm in the list created by the previous call, starting with 0 (zero).\
  For example, the first farm of the contract:\
  `$ near view ${STAKINGCONTRACT} get_unclaimed_reward '{"account_id":"'${MYACCOUNT}'", "farm_id":0}'`
  3. If the result of the previous call is greater than zero, you have earned tokens! Here's how to withdraw them. 

Each farm has a field "token_id" which refers to the fungible token contract that issued the tokens in the first place.
It is important that you have storage in this contract to be able to receive tokens from it. For example, if the
token_id is "token.example.testnet" you will have to create storage like this:

`$ near call token.example.testnet storage_deposit '' --accountId ${OWNERACC} --amount 0.00125`

Afterwards you can claim your rewards:

`$ near call ${STAKINGCONTRACT} claim '{"account_id": "'${MYACCOUNT}'", "token_id": "token.example.testnet", "farm_id": 0}' --accountId ${OWNERACC} --gas 100000000000000 --depositYocto 1`

This will transfer the tokens earned from the first farm (farm_id:0) to your account. Please make sure that token_id and farm_id exactly match
what was returned by the previous "get_farms" call.

The "claim" call can be expensive, resulting in an error that signals that not enough gas was available. Increaste the gas parameter in that case and try again.

# Frequently Asked Questions

[TBD]

