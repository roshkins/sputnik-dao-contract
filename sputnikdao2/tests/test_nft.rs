
use near_contract_standards::non_fungible_token::Token;
use near_sdk::json_types::U128;
use near_sdk::AccountId;
use near_sdk_sim::{ContractAccount, call, to_yocto, view};
use test_nft_token::ContractContract as TestNFTContract;
use test_nft_token::sample_token_metadata;
use sputnik_nft_staking::ContractContract as NFTStakingContract;


use crate::utils::*;
use sputnik_nft_staking::User;

use sputnikdao2::{
    Action, Policy, Proposal, ProposalInput, ProposalKind, ProposalStatus, RoleKind,
    RolePermission, VersionedPolicy, VotePolicy,
};



mod utils;

fn user(id: u32) -> AccountId {
    format!("user{}", id).parse().unwrap()
}


const TEST_NFT: &str = "TEST_NFT";


#[test]
fn test_create_dao_and_use_nft() {
    let (root, dao) = setup_dao();
    let user2 = root.create_user(user(2), to_yocto("1000"));
    let user3 = root.create_user(user(3), to_yocto("1000"));
use crate::utils::*;
    let test_nft: ContractAccount<TestNFTContract> = setup_test_nft(&root);
    let staking: ContractAccount<NFTStakingContract> = setup_staking_nft(&root);

    assert!(view!(dao.get_staking_contract())
        .unwrap_json::<String>()
        .as_str()
        .is_empty());
    add_member_proposal(&root, &dao, user2.account_id.clone()).assert_success();
    assert_eq!(view!(dao.get_last_proposal_id()).unwrap_json::<u64>(), 1);
    // Voting by user who is not member should fail.
    should_fail(call!(user2, dao.act_proposal(0, Action::VoteApprove, None)));
    call!(root, dao.act_proposal(0, Action::VoteApprove, None)).assert_success();
    // voting second time should fail.
    should_fail(call!(root, dao.act_proposal(0, Action::VoteApprove, None)));
    // Add 3rd member.
    add_member_proposal(&user2, &dao, user3.account_id.clone()).assert_success();
    vote(vec![&root, &user2], &dao, 1);
    let policy = view!(dao.get_policy()).unwrap_json::<Policy>();
    assert_eq!(policy.roles.len(), 2);
    assert_eq!(
        policy.roles[1].kind,
        RoleKind::Group(
            vec![
                root.account_id.clone(),
                user2.account_id.clone(),
                user3.account_id.clone()
            ]
            .into_iter()
            .collect()
        )
    );
    add_proposal(
        &user2,
        &dao,
        ProposalInput {
            description: "test".to_string(),
            kind: ProposalKind::SetStakingContract {
                staking_id: "staking".parse().unwrap(),
            },
        },
    )
    .assert_success();
    vote(vec![&user3, &user2], &dao, 2);
    assert!(!view!(dao.get_staking_contract())
        .unwrap_json::<AccountId>()
        .as_str()
        .is_empty());
    assert_eq!(
        view!(dao.get_proposal(2)).unwrap_json::<Proposal>().status,
        ProposalStatus::Approved
    );

    // Test staking, starting with a zero amount staked.
    staking
        .user_account
        .view_method_call(staking.contract.nft_total_supply());
    assert_eq!(
        view!(staking.nft_total_supply()).unwrap_json::<U128>().0,
        to_yocto("0")
    );

    // Mint nft TEST_NFT and give to to user2
     call!(
        root,
        test_nft.nft_mint(
            TEST_NFT.to_string(),
            user2.account_id.clone(),
            sample_token_metadata()
        ),
       deposit = 6220000000000000000000
    )
    .assert_success();
    
    // Transfer nft to staking contract.
    call!(
        user2,
        test_nft.nft_transfer_call(
            staking.account_id(),
            TEST_NFT.to_string(),
            None,
            None,
            "".to_string()
        ),
        deposit = 1
    )
    .assert_success();

    //NFT should be in contract
    assert_eq!(view!(staking.nft_total_supply()).unwrap_json::<U128>().0, 1);

    // Check user2's balance went up on staking contract
    let user2_id = user2.account_id.clone();
    assert_eq!(
        view!(staking.nft_balance_of(user2_id.clone()))
            .unwrap_json::<U128>()
            .0,
        1
    );

    // Ownership of NFT should transfer
    assert_eq!(
        view!(test_nft.nft_token(TEST_NFT.to_string()))
            .unwrap_json::<Token>()
            .owner_id,
        staking.account_id()
    );

    // Withdraw the NFT back.
    call!(user2, staking.withdraw(TEST_NFT.to_string(), U128(1))).assert_success();
    assert_eq!(view!(staking.nft_total_supply()).unwrap_json::<U128>().0, 0);
    assert_eq!(
        view!(test_nft.nft_token(TEST_NFT.to_string()))
            .unwrap_json::<Token>()
            .owner_id,
        user2_id.clone()
    );

    // Can delegate token to self
    call!(
        user2,
        staking.delegate(user2_id.clone(), TEST_NFT.to_string(), U128(1))
    )
    .assert_success();
    call!(
        user2,
        staking.undelegate(user2_id.clone(), TEST_NFT.to_string(), U128(1))
    )
    .assert_success();
    // should fail right after undelegation as need to wait for voting period before can delegate again.
    should_fail(call!(
        user2,
        staking.delegate(user2_id.clone(), TEST_NFT.to_string(), U128(1))
    ));

    let user = view!(staking.get_user(user2_id.clone())).unwrap_borsh::<User>();
    assert_eq!(
        user.delegated_amounts,
        vec![]
    );
    assert_eq!(
        view!(dao.delegation_total_supply()).unwrap_json::<U128>().0,
        0
    );
    assert_eq!(
        view!(dao.delegation_balance_of(user2_id.clone()))
            .unwrap_json::<U128>()
            .0,
        0
    );
}

/// Test various cases that must fail.
#[test]
fn test_failures() {
    let (root, dao) = setup_dao();
    should_fail(add_transfer_proposal(
        &root,
        &dao,
        base_token(),
        user(1),
        1_000_000,
        Some("some".to_string()),
    ));
    should_fail(add_transfer_proposal(
        &root,
        &dao,
        "not:a^valid.token@".parse().unwrap(),
        user(1),
        1_000_000,
        None,
    ));
}
