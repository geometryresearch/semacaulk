use std::{cmp::max, marker::PhantomData};

use ark_ec::{AffineCurve, PairingEngine, ProjectiveCurve};
use ark_ff::{to_bytes, Field, One, PrimeField, Zero};
use ark_poly::{
    univariate::DensePolynomial, EvaluationDomain, GeneralEvaluationDomain, Polynomial,
    UVPolynomial,
};
use ark_std::{cfg_into_iter, rand::RngCore, UniformRand};

use crate::{
    kzg::{commit, open},
    rng::FiatShamirRng,
    utils::{construct_lagrange_basis, shift_dense_poly},
};

use super::{
    precomputed::Precomputed, proof::Proof, verifier::VerifierMessages, CommonInput, PublicInput,
};

pub struct WitnessInput<F: Field> {
    pub(crate) indices: Vec<usize>,
    pub(crate) values: Vec<F>,
    pub(crate) _c: DensePolynomial<F>,
    pub(crate) a: DensePolynomial<F>,
    pub(crate) mapping: Vec<usize>,
}

struct State<'a, E: PairingEngine> {
    // init data in the state
    pub(crate) public_input: &'a PublicInput<E>,
    pub(crate) common_input: &'a CommonInput<E>,
    pub(crate) witness: &'a WitnessInput<E::Fr>,
    pub(crate) precomputed: &'a Precomputed<E>,
    pub(crate) shifted_a: DensePolynomial<E::Fr>,

    // data after first round
    pub(crate) r1: Option<E::Fr>,
    pub(crate) r2: Option<E::Fr>,
    pub(crate) r3: Option<E::Fr>,
    pub(crate) r4: Option<E::Fr>,
    pub(crate) r5: Option<E::Fr>,
    pub(crate) r6: Option<E::Fr>,

    pub(crate) zi: Option<DensePolynomial<E::Fr>>,
    pub(crate) ci: Option<DensePolynomial<E::Fr>>,
    pub(crate) u: Option<DensePolynomial<E::Fr>>,

    // data after second round
    pub(crate) zi_of_ui: Option<DensePolynomial<E::Fr>>,
    pub(crate) ci_of_ui: Option<DensePolynomial<E::Fr>>,
    pub(crate) h: Option<DensePolynomial<E::Fr>>,
}

pub struct Prover<E: PairingEngine> {
    _pe: PhantomData<E>,
}

impl<E: PairingEngine> Prover<E> {
    pub fn prove<R: RngCore>(
        public_input: &PublicInput<E>,
        common_input: &CommonInput<E>,
        witness: &WitnessInput<E::Fr>,
        precomputed: &Precomputed<E>,
        zk_rng: &mut R,
        fs_rng: &mut impl FiatShamirRng, // Since we use caulk+ as subprotocol, at this moment transcript is already initialized
    ) -> (Proof<E>, E::Fr) {
        let mut state = Self::init(public_input, common_input, witness, precomputed);
        let mut verifier_msgs = VerifierMessages::<E::Fr>::empty();

        // first round
        let (zi_commitment, ci_commitment, u_commitment) = Self::first_round(&mut state, zk_rng);
        fs_rng.absorb(&to_bytes![&zi_commitment, &ci_commitment, &u_commitment].unwrap());
        verifier_msgs.first_msg(fs_rng);

        // second round
        let (w_commitment, h_commitment) = Self::second_round(&mut state, &verifier_msgs);
        fs_rng.absorb(&to_bytes![&w_commitment, &h_commitment].unwrap());
        verifier_msgs.second_msg(fs_rng);

        // third round
        let (u_prime_eval, u_prime_proof, p1_eval, p1_proof, p2_proof) =
            Self::third_round(&state, &verifier_msgs);
        fs_rng.absorb(
            &to_bytes![&u_prime_eval, &u_prime_proof, p1_eval, p1_proof, p2_proof].unwrap(),
        );

        let proof = Proof {
            zi_commitment,
            ci_commitment,
            u_commitment,
            w_commitment,
            h_commitment,
            u_prime_eval,
            u_prime_proof,
            p1_eval,
            p1_proof,
            p2_proof,
        };

        // This is temporary until it's integrated with upper quotient protocol
        let alpha = verifier_msgs.alpha.as_ref().unwrap();
        let a_opening_at_rotation = state.shifted_a.evaluate(alpha);
        (proof, a_opening_at_rotation)
    }

    fn init<'a>(
        public_input: &'a PublicInput<E>,
        common_input: &'a CommonInput<E>,
        witness: &'a WitnessInput<E::Fr>,
        precomputed: &'a Precomputed<E>,
    ) -> State<'a, E> {
        let omega_pow_rotation = common_input.domain_h.element(common_input.rotation);
        let shifted_a = shift_dense_poly(&witness.a, &omega_pow_rotation);
        State {
            public_input,
            common_input,
            witness,
            precomputed,
            shifted_a,

            r1: None,
            r2: None,
            r3: None,
            r4: None,
            r5: None,
            r6: None,

            zi: None,
            ci: None,
            u: None,

            zi_of_ui: None,
            ci_of_ui: None,
            h: None,
        }
    }

    fn first_round<R: RngCore>(
        state: &mut State<E>,
        rng: &mut R,
    ) -> (E::G1Affine, E::G1Affine, E::G1Affine) {
        // 1. sample blinding factors
        let r1 = E::Fr::rand(rng);
        let r2 = E::Fr::rand(rng);
        let r3 = E::Fr::rand(rng);
        let r4 = E::Fr::rand(rng);
        let r5 = E::Fr::rand(rng);
        let r6 = E::Fr::rand(rng);

        state.r1 = Some(r1);
        state.r2 = Some(r2);
        state.r3 = Some(r3);
        state.r4 = Some(r4);
        state.r5 = Some(r5);
        state.r6 = Some(r6);

        // 2. compute lagrange basis polynomials t_i over w^j for j in I
        let elems: Vec<E::Fr> = state
            .witness
            .indices
            .iter()
            .map(|&i| state.common_input.domain_h.element(i))
            .collect();
        let ts = construct_lagrange_basis(&elems);

        // 3. define and mask zI`
        let mut zi = DensePolynomial::<E::Fr>::from_coefficients_slice(&[r1]);
        for omega_i in elems {
            zi = &zi * &DensePolynomial::from_coefficients_slice(&[-omega_i, E::Fr::one()]);
        }

        // 4. define CI
        let mut ci = DensePolynomial::<E::Fr>::zero();
        for (t_i, &eval) in ts.iter().zip(state.witness.values.iter()) {
            ci += &(t_i * eval)
        }

        // 5. blind CI
        let ci_blind = &DensePolynomial::from_coefficients_slice(&[r2, r3, r4]) * &zi;
        ci += &ci_blind;

        // 6. define U
        let u_prime_evals = (0..state.common_input.domain_v.size())
            .map(|i| {
                state
                    .common_input
                    .domain_h
                    .element(state.witness.mapping[i])
            })
            .collect::<Vec<_>>();
        let mut u = DensePolynomial::from_coefficients_slice(
            &state.common_input.domain_v.ifft(&u_prime_evals),
        );

        // 7. blind U
        let zv: DensePolynomial<_> = state.common_input.domain_v.vanishing_polynomial().into();
        let u_blind = &DensePolynomial::from_coefficients_slice(&[r5, r6]) * &zv;
        u += &u_blind;

        // 8. Commit
        let zi_commitment = commit(&state.public_input.srs_g1, &zi);
        let ci_commitment = commit(&state.public_input.srs_g1, &ci);
        let u_commitment = commit(&state.public_input.srs_g1, &u);

        // store data in the state
        state.zi = Some(zi);
        state.ci = Some(ci);
        state.u = Some(u);

        (
            zi_commitment.into(),
            ci_commitment.into(),
            u_commitment.into(),
        )
    }

    fn second_round<'a>(
        state: &mut State<'a, E>,
        msgs: &VerifierMessages<E::Fr>,
    ) -> (E::G2Affine, E::G1Affine) {
        let xi_1 = msgs.xi_1.unwrap();
        let xi_2 = msgs.xi_2.unwrap();

        // 1. compute linearly separated quotients in g2
        let mut w1_xi2_w2 = E::G2Projective::zero();
        for i in &state.witness.indices {
            let w1_i = state.precomputed.get_w1_i(i);
            let w2_i = state.precomputed.get_w2_i(i);

            let omega_i = state.common_input.domain_h.element(*i);
            let mut denom = E::Fr::one();
            for j in &state.witness.indices {
                if j != i {
                    denom *= omega_i - state.common_input.domain_h.element(*j);
                }
            }

            let denom_inv = denom.inverse().unwrap();
            w1_xi2_w2 = w1_xi2_w2 + w1_i.mul(denom_inv) + w2_i.mul(denom_inv * xi_2);
        }

        // 2. Compute H
        let zi = state.zi.as_ref().unwrap();
        let ci = state.ci.as_ref().unwrap();
        let u = state.u.as_ref().unwrap();

        let composed_degree = max(zi.degree() * u.degree(), ci.degree() * u.degree());
        let extended_domain = GeneralEvaluationDomain::<E::Fr>::new(composed_degree).unwrap();

        let u_prime_evals_on_extended_domain =
            cfg_into_iter!(extended_domain.elements()).map(|omega_i| u.evaluate(&omega_i));
        let mut zi_of_u_prime_evals = vec![E::Fr::zero(); extended_domain.size()];
        let mut ci_of_u_prime_evals = vec![E::Fr::zero(); extended_domain.size()];
        for (i, ui) in u_prime_evals_on_extended_domain.enumerate() {
            zi_of_u_prime_evals[i] = zi.evaluate(&ui);
            ci_of_u_prime_evals[i] = ci.evaluate(&ui);
        }

        let zi_of_ui =
            DensePolynomial::from_coefficients_slice(&extended_domain.ifft(&zi_of_u_prime_evals));
        let ci_of_ui =
            DensePolynomial::from_coefficients_slice(&extended_domain.ifft(&ci_of_u_prime_evals));

        let num = &zi_of_ui + &(&(&ci_of_ui - &state.shifted_a) * xi_1);
        let (h, r) = num
            .divide_by_vanishing_poly(state.common_input.domain_v)
            .unwrap();

        // sanity
        assert!(r.is_zero());

        // 3. Commit
        let r1 = state.r1.unwrap();
        let r2 = state.r2.unwrap();
        let r3 = state.r3.unwrap();
        let r4 = state.r4.unwrap();

        let ci_blinder = &DensePolynomial::from_coefficients_slice(&[r2, r3, r4]);
        let ci_blinder_commitment = commit(&state.public_input.srs_g2, &ci_blinder);

        let w_commitment = w1_xi2_w2.mul(r1.inverse().unwrap().into_repr()) - ci_blinder_commitment;
        let h_commitment = commit(&state.public_input.srs_g1, &h);

        // store data in the state
        state.zi_of_ui = Some(zi_of_ui);
        state.ci_of_ui = Some(ci_of_ui);
        state.h = Some(h);

        (w_commitment.into(), h_commitment.into())
    }

    fn third_round<'a>(
        state: &State<'a, E>,
        msgs: &VerifierMessages<E::Fr>,
    ) -> (E::Fr, E::G1Affine, E::Fr, E::G1Affine, E::G1Affine) {
        let xi_1 = msgs.xi_1.unwrap();
        let alpha = msgs.alpha.unwrap();

        let zi = state.zi.as_ref().unwrap();
        let ci = state.ci.as_ref().unwrap();
        let u = state.u.as_ref().unwrap();
        let h = state.h.as_ref().unwrap();

        let a_opening_at_rotation = state.shifted_a.evaluate(&alpha);

        // 1. Compute P1
        let p1 = zi + &(ci * xi_1);

        // 2. Compute P2
        let p2 = {
            let u_at_alpha = u.evaluate(&alpha);
            let zi_at_u_alpha = zi.evaluate(&u_at_alpha);
            let ci_at_u_alpha = ci.evaluate(&u_at_alpha);

            let zv_alpha = state
                .common_input
                .domain_v
                .evaluate_vanishing_polynomial(alpha);

            let free_coeff = xi_1 * ci_at_u_alpha + zi_at_u_alpha - xi_1 * a_opening_at_rotation;
            let mut h_zv = h * -zv_alpha;
            h_zv[0] += free_coeff;

            h_zv
        };

        // 3. Open
        let (u_prime_eval, u_prime_proof) = open(&state.public_input.srs_g1, u, alpha);
        let (p1_eval, p1_proof) = open(&state.public_input.srs_g1, &p1, u_prime_eval);
        let (p2_eval, p2_proof) = open(&state.public_input.srs_g1, &p2, alpha);

        // sanity
        assert_eq!(p2_eval, E::Fr::zero());

        (u_prime_eval, u_prime_proof, p1_eval, p1_proof, p2_proof)
    }
}
