use std::convert::TryFrom;

use near_contract_standards::non_fungible_token::core::NonFungibleTokenReceiver;
use near_contract_standards::non_fungible_token::TokenId;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::{U128, U64};
use near_sdk::{
    env, ext_contract, near_bindgen, AccountId, Balance, BorshStorageKey, Duration, Gas,
    PanicOnDefault, Promise, PromiseOrValue, PromiseResult,
};

pub use user::{User, VersionedUser};

mod storage_impl;
mod user;

#[ext_contract(ext_non_fungible_token)]
pub trait NonFungibleTokenCore {
    /// Simple transfer. Transfer a given `token_id` from current owner to
    /// `receiver_id`.
    ///
    /// Requirements
    /// * Caller of the method must attach a deposit of 1 yoctoⓃ for security purposes
    /// * Contract MUST panic if called by someone other than token owner or,
    ///   if using Approval Management, one of the approved accounts
    /// * `approval_id` is for use with Approval Management,
    ///   see https://nomicon.io/Standards/NonFungibleToken/ApprovalManagement.html
    /// * If using Approval Management, contract MUST nullify approved accounts on
    ///   successful transfer.
    /// * TODO: needed? Both accounts must be registered with the contract for transfer to
    ///   succeed. See see https://nomicon.io/Standards/StorageManagement.html
    ///
    /// Arguments:
    /// * `receiver_id`: the valid NEAR account receiving the token
    /// * `token_id`: the token to transfer
    /// * `approval_id`: expected approval ID. A number smaller than
    ///    2^53, and therefore representable as JSON. See Approval Management
    ///    standard for full explanation.
    /// * `memo` (optional): for use cases that may benefit from indexing or
    ///    providing information for a transfer
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    );

    /// Transfer token and call a method on a receiver contract. A successful
    /// workflow will end in a success execution outcome to the callback on the NFT
    /// contract at the method `nft_resolve_transfer`.
    ///
    /// You can think of this as being similar to attaching native NEAR tokens to a
    /// function call. It allows you to attach any Non-Fungible Token in a call to a
    /// receiver contract.
    ///
    /// Requirements:
    /// * Caller of the method must attach a deposit of 1 yoctoⓃ for security
    ///   purposes
    /// * Contract MUST panic if called by someone other than token owner or,
    ///   if using Approval Management, one of the approved accounts
    /// * The receiving contract must implement `ft_on_transfer` according to the
    ///   standard. If it does not, FT contract's `ft_resolve_transfer` MUST deal
    ///   with the resulting failed cross-contract call and roll back the transfer.
    /// * Contract MUST implement the behavior described in `ft_resolve_transfer`
    /// * `approval_id` is for use with Approval Management extension, see
    ///   that document for full explanation.
    /// * If using Approval Management, contract MUST nullify approved accounts on
    ///   successful transfer.
    ///
    /// Arguments:
    /// * `receiver_id`: the valid NEAR account receiving the token.
    /// * `token_id`: the token to send.
    /// * `approval_id`: expected approval ID. A number smaller than
    ///    2^53, and therefore representable as JSON. See Approval Management
    ///    standard for full explanation.
    /// * `memo` (optional): for use cases that may benefit from indexing or
    ///    providing information for a transfer.
    /// * `msg`: specifies information needed by the receiving contract in
    ///    order to properly handle the transfer. Can indicate both a function to
    ///    call and the parameters to pass to that function.
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool>;

    /// Returns the token with the given `token_id` or `null` if no such token.
    fn nft_token(&self, token_id: TokenId) -> Option<Token>;
}

#[derive(BorshStorageKey, BorshSerialize)]
enum StorageKeys {
    Users,
    ValidNFTs,
}

/// Amount of gas for fungible token transfers.
pub const GAS_FOR_NFT_TRANSFER: Gas = Gas(10_000_000_000_000);

/// Amount of gas for delegate action.
pub const GAS_FOR_DELEGATE: Gas = Gas(10_000_000_000_000);

/// Amount of gas for register action.
pub const GAS_FOR_REGISTER: Gas = Gas(10_000_000_000_000);

/// Amount of gas for undelegate action.
pub const GAS_FOR_UNDELEGATE: Gas = Gas(10_000_000_000_000);

#[ext_contract(ext_sputnik)]
pub trait Sputnik {
    fn register_delegation(&mut self, account_id: AccountId);
    fn delegate(&mut self, account_id: AccountId, amount: U128);
    fn undelegate(&mut self, account_id: AccountId, amount: U128);
}

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
pub struct Contract {
    /// DAO owner of this staking contract.
    owner_id: AccountId,
    /// Vote token account.
    token_ids_with_vote_weights: UnorderedMap<String, U128>,
    /// Recording user deposits.
    users: LookupMap<AccountId, VersionedUser>,
    /// Total token amount deposited per token.
    total_amount: UnorderedMap<String, Balance>,
    /// Duration of unstaking. Should be over the possible voting periods.
    unstake_period: Duration,
}

#[ext_contract(ext_self)]
pub trait Contract {
    fn exchange_callback_post_withdraw(
        &mut self,
        sender_id: AccountId,
        token_id: String,
        amount: U128,
    );
}

#[near_bindgen]
impl Contract {
    //TODO: use a Map for token_ids to vote_weights for optimization
    #[init]
    pub fn new(
        #[serializer(borsh)] owner_id: AccountId,
        #[serializer(borsh)] token_ids_with_vote_weights: UnorderedMap<String, U128>,
        #[serializer(borsh)] unstake_period: U64,
        //TODO: Optimize storage, see: https://stackoverflow.com/questions/69096013/how-can-i-serialize-a-near-sdk-rs-lookupmap-that-uses-a-string-as-a-key-or-is-t
    ) -> Self {
        Self {
            owner_id: owner_id.into(),
            token_ids_with_vote_weights,
            users: LookupMap::new(StorageKeys::Users),
            total_amount: UnorderedMap::new(StorageKeys::ValidNFTs),
            unstake_period: unstake_period.0,
        }
    }

    pub fn adopt_new_nfts(
        &mut self,
        #[serializer(borsh)] token_ids_and_weights: UnorderedMap<String, U128>,
    ) {
        let sender_id = env::predecessor_account_id();
        assert!(sender_id == self.owner_id, "ERR_INVALID_APPROVER");
        self.token_ids_with_vote_weights
            .extend(token_ids_and_weights.iter());
    }

    /// Total number of tokens staked in this contract.
    pub fn nft_total_supply(&self) -> U128 {
        let mut sum = 0;
        for i in self.total_amount.iter() {
            sum += i.1;
        }
        U128(sum)
    }

    /// Sum of each token amount times it's voting weight
    pub fn total_voting_power(&self) -> U128 {
        let mut sum = 0;
        for i in self.total_amount.iter() {
            sum += i.1
                * self
                    .token_ids_with_vote_weights
                    .get(&i.0)
                    .unwrap_or(U128(0))
                    .0;
        }
        U128(sum)
    }

    /// Total number of tokens staked by given user.
    pub fn nft_balance_of(&self, account_id: AccountId) -> U128 {
        let mut sum = 0;
        for i in self.internal_get_user(&account_id).vote_amounts.iter() {
            sum += i.1.0; //Get second field, then get unwrapped number.
        }
        U128(sum)
    }

    /// Returns user information.
    pub fn get_user(&self, account_id: AccountId) -> User {
        self.internal_get_user(&account_id)
    }

    /// Delegate give amount of votes to given account.
    /// If enough tokens and storage, forwards this to owner account.
    pub fn delegate(&mut self, account_id: AccountId, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_delegate(
            sender_id,
            account_id.clone().into(),
            token_id.clone(),
            amount.0,
        );
        ext_sputnik::delegate(
            account_id.into(),
            U128(
                amount.0
                    * self
                        .token_ids_with_vote_weights
                        .get(&token_id.clone())
                        .unwrap_or(U128(0))
                        .0,
            ),
            self.owner_id.clone(),
            0,
            GAS_FOR_DELEGATE,
        )
    }

    /// Remove given amount of delegation.
    pub fn undelegate(&mut self, account_id: AccountId, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_undelegate(
            sender_id,
            account_id.clone().into(),
            token_id.clone(),
            amount.0,
        );
        ext_sputnik::undelegate(
            account_id.into(),
            U128(
                amount.0
                    * self
                        .token_ids_with_vote_weights
                        .get(&token_id.clone())
                        .unwrap_or(U128(0))
                        .0,
            ),
            self.owner_id.clone(),
            0,
            GAS_FOR_UNDELEGATE,
        )
    }

    /// Withdraw non delegated tokens back to the user's account.
    /// If user's account is not registered, will keep funds here.
    pub fn withdraw(&mut self, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_withdraw(&sender_id, token_id.clone(), amount.0);

        ext_non_fungible_token::nft_transfer(
            sender_id.clone(),
            token_id.clone(),
            Some(0),
            None,
            AccountId::try_from(token_id.clone()).unwrap(),
            1,
            GAS_FOR_NFT_TRANSFER,
        )
        .then(ext_self::exchange_callback_post_withdraw(
            sender_id,
            token_id,
            amount,
            env::current_account_id(),
            0,
            GAS_FOR_NFT_TRANSFER,
        ))
    }

    #[private]
    pub fn exchange_callback_post_withdraw(
        &mut self,
        sender_id: AccountId,
        token_id: String,
        amount: U128,
    ) {
        assert_eq!(
            env::promise_results_count(),
            1,
            "ERR_CALLBACK_POST_WITHDRAW_INVALID",
        );
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(_) => {}
            PromiseResult::Failed => {
                // This reverts the changes from withdraw function.
                self.internal_deposit(&sender_id, token_id, amount.0);
            }
        };
    }
}

#[near_bindgen]
impl NonFungibleTokenReceiver for Contract {
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        _previous_owner_id: AccountId,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        msg: String,
    ) -> PromiseOrValue<bool> {
        assert!(
            self.token_ids_with_vote_weights
                .get(&env::predecessor_account_id().as_str().to_string())
                != None,
            "ERR_INVALID_TOKEN"
        );
        assert!(msg.is_empty(), "ERR_INVALID_MESSAGE");

        self.internal_deposit(&sender_id, token_id.clone(), 1);
        PromiseOrValue::Value(false)
    }
}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;

    use near_contract_standards::storage_management::StorageManagement;
    use near_sdk::json_types::U64;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    use near_sdk_sim::to_yocto;

    use super::*;

    #[derive(BorshStorageKey, BorshSerialize)]
    enum StorageKeys {
        NFTs,
    }
    #[test]
    fn test_basics() {
        let period = 1000;
        let mut context = VMContextBuilder::new();

        //Switch to using account 0
        testing_env!(context.predecessor_account_id(accounts(0)).build());

        // Create a staking contract with account 0 as owner (perhaps Sputnikv2 contract)
        // with token accounts 1 and 4, 5 as example NFT token ids, NFT1 weighted 2, NFT4 weighted 7, NFT5 weighted 0 (for error checking).
        let mut nft_ids_and_weights = UnorderedMap::new(StorageKeys::NFTs);
        let nft1 = accounts(1);
        let nft4 = accounts(4);
        let nft5 = accounts(5);
        nft_ids_and_weights.insert(&nft1.to_string(), &U128(2));
        nft_ids_and_weights.insert(&nft4.to_string(), &U128(7));
        nft_ids_and_weights.insert(&nft5.to_string(), &U128(0));

        let mut contract = Contract::new(accounts(0), nft_ids_and_weights, U64(period));

        // Store 1 yoctoⓃ per user testing account for storage deposit
        testing_env!(context.attached_deposit(to_yocto("1")).build());
        contract.storage_deposit(Some(accounts(2)), None);
        testing_env!(context.attached_deposit(to_yocto("1")).build());
        contract.storage_deposit(Some(accounts(3)), None);

        //Create NFTs
        testing_env!(context.predecessor_account_id(nft1.clone()).build());
        testing_env!(context.predecessor_account_id(nft4.clone()).build());
        testing_env!(context.predecessor_account_id(nft5.clone()).build());

        // Send NFTs to staking contract.
        contract.nft_on_transfer(accounts(2), accounts(2), nft1.to_string(), "".to_string());
        contract.nft_on_transfer(accounts(2), accounts(2), nft4.to_string(), "".to_string());
        contract.nft_on_transfer(accounts(2), accounts(2), nft5.to_string(), "".to_string());

        // See 3 tokens deposited.
        assert_eq!(contract.nft_total_supply().0, 3);
        assert_eq!(contract.nft_balance_of(accounts(2)).0, 3);

        // See 9 voting tokens
        assert_eq!(contract.total_voting_power().0, 9);

        let user = contract.get_user(accounts(2));
        assert_eq!(
            user.get_vote_amount(&contract.token_ids_with_vote_weights),
            9
        );
        // Switch to account 2
        testing_env!(context.predecessor_account_id(accounts(2)).build());

        // Withdraw nft4 and check that balance went down.
        contract.withdraw(nft4.to_string(), U128(1));
        assert_eq!(contract.nft_total_supply().0, 2);
        assert_eq!(contract.nft_balance_of(accounts(2)).0, 2);

        // Check voting count went down.
        assert_eq!(contract.total_voting_power().0, 2);
        assert_eq!(
            user.get_vote_amount(&contract.token_ids_with_vote_weights),
            2
        );

        // Delegate voting nft to account 3
        contract.delegate(accounts(3), nft1.to_string(), U128(1));

        // See that user2 has delegated nft1
        let user = contract.get_user(accounts(2));
        assert_eq!(user.delegated_amount(nft1.to_string()), 1);

        // Switch to account 2
        // testing_env!(context.predecessor_account_id(accounts(2)).build());

        // Undelegate nft1
        contract.undelegate(accounts(3), nft1.to_string(), U128(1));

        // See that it was succesfully undelegated
        let user = contract.get_user(accounts(2));
        assert_eq!(user.delegated_amount(nft1.to_string()), 0);

        // User 2 has correct voting power.
        assert_eq!(
            user.get_vote_amount(&contract.token_ids_with_vote_weights),
            2
        );

        //Approve additional nft, only if predecessor is owner.

        //Switch to using account 0
        testing_env!(context.predecessor_account_id(accounts(0)).build());
        let nft6 = "NFT_6".to_string();
        let mut new_tokens_with_weights = UnorderedMap::new(StorageKeys::NFTs);
        new_tokens_with_weights.insert(&nft6, &U128(22));
        contract.adopt_new_nfts(new_tokens_with_weights);

        assert_eq!(
            contract
                .token_ids_with_vote_weights
                .get(&nft6.to_string())
                .unwrap()
                .0,
            22,
            "Token added improperly."
        );

        //Switch to using account 2
        testing_env!(context.predecessor_account_id(accounts(2)).build());

        // account 2 can't singlehandedly adopt an nft
        let result = catch_unwind(|| {
            let mut contract = Contract::new(
                accounts(0),
                UnorderedMap::new(StorageKeys::NFTs),
                U64(period),
            );
            let nft7 = "NFT_7".to_string();
            let mut new_tokens_with_weights_err = UnorderedMap::new(StorageKeys::NFTs);
            new_tokens_with_weights_err.insert(&nft7, &U128(22));
            contract.adopt_new_nfts(new_tokens_with_weights_err);
        });

        assert!(result.is_err(), "nft adopted when it shouldn't");

        // Check that a next_action_timestamp exists
        assert_eq!(user.next_action_timestamp, U64(period));
    }
}
