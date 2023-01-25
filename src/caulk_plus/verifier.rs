use std::{iter, marker::PhantomData};

use ark_ec::{AffineCurve, PairingEngine, ProjectiveCurve};
use ark_ff::{to_bytes, Field, One, PrimeField, Zero};
use ark_poly::EvaluationDomain;
use ark_std::{rand::RngCore, UniformRand};

use crate::{error::Error, rng::FiatShamirRng};

use super::{proof::Proof, CommonInput, PublicInput};

pub struct VerifierMessages<F: Field> {
    pub xi_1: Option<F>,
    pub xi_2: Option<F>,
    pub alpha: Option<F>,
}

impl<F: Field> VerifierMessages<F> {
    pub fn empty() -> Self {
        Self {
            xi_1: None,
            xi_2: None,
            alpha: None,
        }
    }
    pub fn first_msg<R: RngCore>(&mut self, rng: &mut R) {
        self.xi_1 = Some(F::rand(rng));
        self.xi_2 = Some(F::rand(rng));
    }

    pub fn second_msg<R: RngCore>(&mut self, rng: &mut R) {
        self.alpha = Some(F::rand(rng));
    }
}

struct EvaluationProof<E: PairingEngine> {
    p: E::G1Affine,
    q: E::G1Affine,
    opening_challenge: E::Fr,
    opening: E::Fr,
}

fn batch_evaluation_proof_pairings<E: PairingEngine>(
    evaluation_proofs: &[EvaluationProof<E>],
    u: E::Fr,
    u_first: E::Fr, // Do not start with u^0 by default, maybe there are pre-batched checks
) -> (E::G1Affine, E::G1Affine) {
    let powers_of_u = iter::successors(Some(u_first), |u_pow| Some(*u_pow * u));

    let mut lhs = E::G1Projective::zero();
    let mut rhs = E::G1Projective::zero();

    // TODO: mul by u at the end
    for (eval_proof, u_pow) in evaluation_proofs.iter().zip(powers_of_u) {
        let u_pow_rep = u_pow.into_repr();
        lhs += eval_proof.q.mul(u_pow_rep);

        let rhs_i = {
            let q_part = eval_proof
                .q
                .mul((eval_proof.opening_challenge * u_pow).into_repr());
            let p_part = eval_proof.p.mul(u_pow_rep);
            let y_part = E::G1Affine::prime_subgroup_generator()
                .mul((eval_proof.opening * u_pow).into_repr());

            q_part + p_part - y_part
        };

        rhs += rhs_i
    }

    (lhs.into_affine(), rhs.into_affine())
}

pub struct Verifier<E: PairingEngine> {
    _e: PhantomData<E>,
}

impl<E: PairingEngine> Verifier<E> {
    pub fn verify(
        public_input: &PublicInput<E>,
        common_input: &CommonInput<E>,
        proof: &Proof<E>,
        a_opening_at_rotation: E::Fr,
        fs_rng: &mut impl FiatShamirRng,
    ) -> Result<(), Error> {
        let mut verifier_msgs = VerifierMessages::<E::Fr>::empty();
        fs_rng.absorb(
            &to_bytes![
                &proof.zi_commitment,
                &proof.ci_commitment,
                &proof.u_commitment
            ]
            .unwrap(),
        );
        verifier_msgs.first_msg(fs_rng);

        fs_rng.absorb(&to_bytes![&proof.w_commitment, &proof.h_commitment].unwrap());
        verifier_msgs.second_msg(fs_rng);

        fs_rng.absorb(
            &to_bytes![
                &proof.u_prime_eval,
                &proof.u_prime_proof,
                proof.p1_eval,
                proof.p1_proof,
                proof.p2_proof
            ]
            .unwrap(),
        );

        // TODO: create get methods which return error if some msm is None
        let xi_1 = verifier_msgs.xi_1.as_ref().unwrap();
        let xi_2 = verifier_msgs.xi_2.as_ref().unwrap();
        let alpha = verifier_msgs.alpha.as_ref().unwrap();

        //0. compute u opening proof
        let u_prime_proof = EvaluationProof::<E> {
            p: proof.u_commitment,
            q: proof.u_prime_proof,
            opening_challenge: *alpha,
            opening: proof.u_prime_eval,
        };

        // 1. compute p1 & opening proof
        let p1 = proof.zi_commitment + proof.ci_commitment.mul(xi_1.into_repr()).into();
        let p1_proof = EvaluationProof::<E> {
            p: p1,
            q: proof.p1_proof,
            opening_challenge: proof.u_prime_eval,
            opening: proof.p1_eval,
        };

        // 2. compute p2 & opening proof
        let zv_at_alpha = common_input.domain_v.evaluate_vanishing_polynomial(*alpha);
        let p2 = E::G1Affine::prime_subgroup_generator()
            .mul(proof.p1_eval - *xi_1 * a_opening_at_rotation)
            - proof.h_commitment.mul(zv_at_alpha);

        let p2_proof = EvaluationProof::<E> {
            p: p2.into_affine(),
            q: proof.p2_proof,
            opening_challenge: *alpha,
            opening: E::Fr::zero(),
        };

        // 3. check openings
        let ci_and_zh: E::G1Affine = {
            // TODO: [x^n - 1] can be precomputed
            common_input.c_commitment
                + -proof.ci_commitment
                + (public_input.srs_g1[common_input.domain_h.size()]
                    + -E::G1Affine::prime_subgroup_generator())
                .mul(xi_2.into_repr())
                .into()
        };

        // TODO: this to can be precomputed
        let g2_gen: E::G2Prepared = E::G2Affine::prime_subgroup_generator().into();
        let x_g2: E::G2Prepared = public_input.srs_g2[1].into();

        let u = E::Fr::rand(fs_rng);
        let evaluation_proofs = &[u_prime_proof, p1_proof, p2_proof];
        let (lhs_kzg_batched, rhs_kzg_batched) =
            batch_evaluation_proof_pairings(evaluation_proofs, u, u);

        let res = E::product_of_pairings(&[
            ((-lhs_kzg_batched).into(), x_g2),
            ((rhs_kzg_batched + -ci_and_zh).into(), g2_gen),
            (proof.zi_commitment.into(), proof.w_commitment.into()),
        ]);
        if res != E::Fqk::one() {
            return Err(Error::FinalPairingCheckFailed);
        }

        Ok(())
    }
}
