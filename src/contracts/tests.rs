use crate::accumulator::{
    commit_to_lagrange_bases, compute_lagrange_tree, compute_zero_leaf, Accumulator,
};
use crate::kzg::unsafe_setup_g1;
use crate::transcript::Transcript;
use crate::{
    bn_solidity_utils::{f_to_u256, formate_g1, formate_g2},
    keccak_tree::{flatten_proof, KeccakTree},
};
use ark_bn254::{Bn254, Fq, Fq12, Fr, G1Affine, G2Affine};
use ark_ec::AffineCurve;
use ark_ec::PairingEngine;
use ark_ec::ProjectiveCurve;
use ark_ff::BigInteger256;
use ark_ff::Field;
use ark_ff::One;
use ark_ff::Zero;
use ark_ff::{PrimeField, UniformRand};
use ark_std::{rand::rngs::StdRng, test_rng};
use ethers::contract::abigen;
use ethers::core::k256::ecdsa::SigningKey;
use ethers::core::types::U256;
use ethers::core::utils::{hex, keccak256};
use ethers::middleware::SignerMiddleware;
use ethers::providers::{Http, Provider};
use ethers::utils::AnvilInstance;
use ethers::{prelude::*, utils::Anvil};
use std::{convert::TryFrom, sync::Arc, time::Duration};
use tokio::test;

abigen!(KeccackMT, "./src/contracts/out/KeccakMT.sol/KeccakMT.json",);
abigen!(
    Semacaulk,
    "./src/contracts/out/Semacaulk.sol/Semacaulk.json",
);

type EthersClient = Arc<SignerMiddleware<Provider<Http>, Wallet<SigningKey>>>;
type SemacaulkContract = semacaulk::Semacaulk<
    SignerMiddleware<
        ethers::providers::Provider<Http>,
        ethers::signers::Wallet<ethers::core::k256::ecdsa::SigningKey>,
    >,
>;

pub async fn setup_eth_backend() -> (AnvilInstance, EthersClient) {
    // Launch anvil
    let anvil = Anvil::new().spawn();

    // Instantiate the wallet
    let wallet: LocalWallet = anvil.keys()[0].clone().into();

    // Connect to the network
    let provider = Provider::<Http>::try_from(anvil.endpoint())
        .unwrap()
        .interval(Duration::from_millis(10u64));

    // Instantiate the client with the wallet
    let client = Arc::new(SignerMiddleware::new(
        provider,
        wallet.with_chain_id(anvil.chain_id()),
    ));

    (anvil, client)
}

#[test]
pub async fn test_u256_conversion() {
    let mut rng = test_rng();

    let f = Fr::rand(&mut rng);
    let f_converted = f_to_u256(f);

    let repr = f.into_repr().0;
    assert_eq!(f_converted.0, repr);

    let f_back = Fr::from_repr(BigInteger256::new(f_converted.0)).unwrap();
    assert_eq!(f_back, f);
}

#[test]
pub async fn test_keccak_256() {
    // preimage = abi.encode[bytes32(0), bytes32(0)]
    let preimage = [0u8; 64];
    let hash = keccak256(preimage);
    assert_eq!(
        hex::encode(hash),
        "ad3228b676f7d3cd4284a5443f17f1962b36e491b30a40b2405849e597ba5fb5"
    );

    let mut preimage = Vec::from(hash);
    let mut x = preimage.clone();
    preimage.append(&mut x);
    let r2 = keccak256(preimage);
    assert_eq!(
        hex::encode(r2),
        "b4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d30"
    );
}

#[tokio::test]
pub async fn test_keccak_mt() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    // Deploy contract
    let keccak_mt_contract = KeccackMT::deploy(client, ()).unwrap().send().await.unwrap();

    let mut tree = KeccakTree::new(4, [0; 32]);

    for index in 0..tree.num_leaves() {
        let mut leaf = [0u8; 32];
        leaf[31] = index as u8;
        tree.set(index, leaf);
    }

    for index in 0..tree.num_leaves() {
        let proof = tree.proof(index).unwrap();
        let flattened_proof = flatten_proof(&proof);

        let leaf = tree.leaves()[index];

        // Call the contract function
        let index = U256::from(index);
        let result = keccak_mt_contract
            .gen_root_from_path(index, leaf, flattened_proof)
            .call()
            .await
            .unwrap();
        assert_eq!(hex::encode(tree.root()), hex::encode(result));
    }

    drop(anvil);
}

pub async fn deploy_semacaulk(
    domain_size: usize,
    rng: &mut StdRng,
    client: EthersClient,
) -> (SemacaulkContract, Accumulator<Bn254>) {
    let zero = compute_zero_leaf::<Fr>();
    let srs_g1 = unsafe_setup_g1::<Bn254, StdRng>(domain_size, rng);

    let lagrange_comms = commit_to_lagrange_bases::<Bn254>(domain_size, &srs_g1);

    let acc = Accumulator::<Bn254>::new(zero, &lagrange_comms);

    let empty_accumulator_x = f_to_u256::<Fq>(acc.point.x);
    let empty_accumulator_y = f_to_u256::<Fq>(acc.point.y);

    // Construct the tree of commitments to the Lagrange bases
    let tree = compute_lagrange_tree::<Bn254>(&lagrange_comms);
    let root = tree.root();

    // Deploy contract
    let semacaulk_contract =
        Semacaulk::deploy(client, (root, empty_accumulator_x, empty_accumulator_y))
            .unwrap()
            .send()
            .await
            .unwrap();

    (semacaulk_contract, acc)
}

#[tokio::test]
pub async fn test_semacaulk_insert() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;
    let mut acc = semacaulk_contract_and_acc.1;

    let tree = compute_lagrange_tree::<Bn254>(&acc.lagrange_comms);

    for index in 0..tree.num_leaves() {
        let proof = tree.proof(index).unwrap();
        let flattened_proof = flatten_proof(&proof);

        let l_i = acc.lagrange_comms[index];
        let l_i_x = f_to_u256(l_i.x);
        let l_i_y = f_to_u256(l_i.y);

        let new_leaf = Fr::rand(&mut rng);
        let new_leaf_u256 = f_to_u256(new_leaf);

        println!("index: {}", index);

        // Insert the leaf on chain
        let result = semacaulk_contract
            .insert_identity(new_leaf_u256, l_i_x, l_i_y, flattened_proof)
            .send()
            .await
            .unwrap()
            .await
            .unwrap()
            .expect("no receipt found");

        //println!("{:?}", result);
        assert_eq!(result.status.unwrap(), ethers::types::U64::from(1));

        println!(
            "Gas used by insertIdentity(): {:?}",
            result.gas_used.unwrap()
        );

        // Check that currentIndex is incremented
        let new_index = semacaulk_contract.get_current_index().call().await.unwrap();
        assert_eq!(new_index, U256::from(index + 1));

        // Insert the leaf off-chain
        acc.update(index, new_leaf);

        let onchain_point = semacaulk_contract.get_accumulator().call().await.unwrap();
        assert_eq!(f_to_u256(acc.point.x), onchain_point.x);
        assert_eq!(f_to_u256(acc.point.y), onchain_point.y);
    }

    drop(anvil);
}

#[tokio::test]
pub async fn test_pairing() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    // Pairing tests that: e(-a1, a2) * e(b1, b2) * e(c2, c3) == 1

    let a2 = Fr::rand(&mut rng);

    let b1 = Fr::rand(&mut rng);
    let b2 = Fr::rand(&mut rng);

    let c1 = Fr::rand(&mut rng);
    let c2 = Fr::rand(&mut rng);

    let a1 = (b1 * b2 + c1 * c2) * a2.inverse().unwrap();

    // Sanity 1
    assert_eq!(-a1 * a2 + b1 * b2 + c1 * c2, Fr::zero());

    let g1 = G1Affine::prime_subgroup_generator();
    let g2 = G2Affine::prime_subgroup_generator();

    let a1 = g1.mul(-a1).into_affine();
    let a2 = g2.mul(a2).into_affine();
    let b1 = g1.mul(b1).into_affine();
    let b2 = g2.mul(b2).into_affine();
    let c1 = g1.mul(c1).into_affine();
    let c2 = g2.mul(c2).into_affine();

    let res = Bn254::product_of_pairings(&[
        (a1.into(), a2.into()),
        (b1.into(), b2.into()),
        (c1.into(), c2.into()),
    ]);

    // Sanity 2
    assert_eq!(res, Fq12::one());

    let result: bool = semacaulk_contract
        .verify_proof(
            formate_g1(a1),
            formate_g2(a2),
            formate_g1(b1),
            formate_g2(b2),
            formate_g1(c1),
            formate_g2(c2),
        )
        .call()
        .await
        .unwrap();

    assert!(result);

    drop(anvil);
}

#[tokio::test]
pub async fn test_transcript() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    let (ch_contract_1, ch_contract_2) =
        semacaulk_contract.verify_transcript().call().await.unwrap();

    let mut transcript = Transcript::new_transcript();

    let u1 = Fr::from(100);
    transcript.update_with_u256(u1);

    let g1 = G1Affine::prime_subgroup_generator();
    transcript.update_with_g1(&g1);

    let challenge_1 = transcript.get_challenge();

    let u2 = Fr::from(200);
    transcript.update_with_u256(u2);

    let challenge_2 = transcript.get_challenge();

    assert_eq!(ch_contract_1, f_to_u256(challenge_1));
    assert_eq!(ch_contract_2, f_to_u256(challenge_2));

    drop(anvil);
}

#[tokio::test]
pub async fn test_id_nullifier_gate_eval() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    // Raises a given field element to the power of 7
    let pow_7 = |x: Fr| x.pow(&[7, 0, 0, 0]);

    let q_mimc = Fr::rand(&mut rng);
    let w0 = Fr::rand(&mut rng);
    let c = Fr::rand(&mut rng);
    let w0gamma = Fr::rand(&mut rng);

    let expected = q_mimc * (pow_7(w0 + c) - w0gamma);

    let result = semacaulk_contract.id_nullifier_gate_eval(
        f_to_u256(q_mimc),
        f_to_u256(w0),
        f_to_u256(c),
        f_to_u256(w0gamma),
    ).call().await.unwrap();

    assert_eq!(result, f_to_u256(expected));
    drop(anvil);
}

#[tokio::test]
pub async fn test_id_comm_lrd_gate_eval() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    // Raises a given field element to the power of 7
    let pow_7 = |x: Fr| x.pow(&[7, 0, 0, 0]);

    let q_mimc = Fr::rand(&mut rng);
    let w1 = Fr::rand(&mut rng);
    let c = Fr::rand(&mut rng);
    let w1gamma = Fr::rand(&mut rng);
    let key = Fr::rand(&mut rng);

    let expected = q_mimc * (pow_7(w1 + key + c) - w1gamma);

    let result = semacaulk_contract.id_comm_lrd_eval(
        f_to_u256(q_mimc),
        f_to_u256(w1),
        f_to_u256(key),
        f_to_u256(c),
        f_to_u256(w1gamma),
    ).call().await.unwrap();

    assert_eq!(result, f_to_u256(expected));
    drop(anvil);
}

#[tokio::test]
pub async fn test_key_constant_gate_eval() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    let q_mimc = Fr::rand(&mut rng);
    let key = Fr::rand(&mut rng);
    let key_gamma = Fr::rand(&mut rng);

    let expected = q_mimc * (key - key_gamma);

    let result = semacaulk_contract.key_constant_eval(
        f_to_u256(q_mimc),
        f_to_u256(key),
        f_to_u256(key_gamma),
    ).call().await.unwrap();

    assert_eq!(result, f_to_u256(expected));
    drop(anvil);
}

#[tokio::test]
pub async fn test_key_copy_gate_eval() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    let l0 = Fr::rand(&mut rng);
    let key = Fr::rand(&mut rng);
    let w0 = Fr::rand(&mut rng);
    let w0_gamma_91 = Fr::rand(&mut rng);

    let expected = l0 * (key - w0 - w0_gamma_91);

    let result = semacaulk_contract.key_copy_eval(
        f_to_u256(l0),
        f_to_u256(key),
        f_to_u256(w0),
        f_to_u256(w0_gamma_91),
    ).call().await.unwrap();

    assert_eq!(result, f_to_u256(expected));
    drop(anvil);
}

#[tokio::test]
pub async fn test_nullifier_hash_final_gate_eval() {
    let eth_backend = setup_eth_backend().await;
    let anvil = eth_backend.0;
    let client = eth_backend.1;

    let domain_size = 8;
    let mut rng = test_rng();

    let semacaulk_contract_and_acc = deploy_semacaulk(domain_size, &mut rng, client).await;
    let semacaulk_contract = semacaulk_contract_and_acc.0;

    let l0 = Fr::rand(&mut rng);
    let nullifier_hash = Fr::rand(&mut rng);
    let key = Fr::rand(&mut rng);
    let w2 = Fr::rand(&mut rng);
    let w2_gamma_91 = Fr::rand(&mut rng);

    let expected = l0 * (nullifier_hash - w2 - w2_gamma_91 - (key * Fr::from(2)));

    let result = semacaulk_contract.nullifier_hash_final_eval(
        f_to_u256(l0),
        f_to_u256(nullifier_hash),
        f_to_u256(w2),
        f_to_u256(w2_gamma_91),
        f_to_u256(key),
    ).call().await.unwrap();

    assert_eq!(result, f_to_u256(expected));
    drop(anvil);
}
