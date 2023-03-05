use std::iter;

use crate::{
    constants::{DUMMY_VALUE, EXTENDED_DOMAIN_FACTOR, NUMBER_OF_MIMC_ROUNDS, SUBGROUP_SIZE},
    multiopen::MultiopenProof,
    utils::compute_vanishing_poly_over_coset,
};
use ark_ec::PairingEngine;
use ark_ff::{One, Zero};
use ark_poly::{
    univariate::DensePolynomial, EvaluationDomain, GeneralEvaluationDomain, UVPolynomial,
};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};

pub mod precomputed;
#[allow(clippy::module_inception)]
pub mod prover;

use crate::prover::precomputed::Precomputed;

#[derive(CanonicalSerialize, CanonicalDeserialize, Clone, Debug)]
pub struct Proof<E: PairingEngine> {
    pub(crate) multiopen_proof: MultiopenProof<E>,
    pub(crate) openings: Openings<E>,
    pub(crate) commitments: Commitments<E>,
}

#[derive(CanonicalSerialize, CanonicalDeserialize, Clone, Debug)]
pub struct Openings<E: PairingEngine> {
    pub(crate) q_mimc: E::Fr,
    pub(crate) mimc_cts: E::Fr,
    pub(crate) quotient: E::Fr,
    pub(crate) u_prime: E::Fr,
    pub(crate) p1: E::Fr,
    pub(crate) p2: E::Fr,
    pub(crate) w0_0: E::Fr,
    pub(crate) w0_1: E::Fr,
    pub(crate) w0_2: E::Fr,
    pub(crate) w1_0: E::Fr,
    pub(crate) w1_1: E::Fr,
    pub(crate) w1_2: E::Fr,
    pub(crate) w2_0: E::Fr,
    pub(crate) w2_1: E::Fr,
    pub(crate) w2_2: E::Fr,
    pub(crate) key_0: E::Fr,
    pub(crate) key_1: E::Fr,
}

#[derive(CanonicalSerialize, CanonicalDeserialize, Clone, Debug)]
pub struct Commitments<E: PairingEngine> {
    pub(crate) w0: E::G1Affine,
    pub(crate) w1: E::G1Affine,
    pub(crate) w2: E::G1Affine,
    pub(crate) key: E::G1Affine,
    pub(crate) mimc_cts: E::G1Affine,
    pub(crate) quotient: E::G1Affine,
    pub(crate) u_prime: E::G1Affine,
    pub(crate) zi: E::G1Affine,
    pub(crate) ci: E::G1Affine,
    pub(crate) p1: E::G1Affine,
    pub(crate) p2: E::G1Affine,
    pub(crate) q_mimc: E::G1Affine,
    pub(crate) h: E::G1Affine,
    pub(crate) w: E::G2Affine,
}

#[derive(CanonicalSerialize, CanonicalDeserialize, Clone, Debug)]
pub struct ProverPrecomputedData<E: PairingEngine> {
    pub(crate) mimc_cts: DensePolynomial<E::Fr>, // mimc round constants poly
    pub(crate) mimc_cts_coset_evals: Vec<E::Fr>, // evaluations of mimc round constants over coset
    pub(crate) zh_inverse_coset_evals: Vec<E::Fr>, // evaluations of vanishing poly over coset
    pub(crate) q_mimc: DensePolynomial<E::Fr>,
    pub(crate) q_mimc_coset_evals: Vec<E::Fr>,
    pub(crate) l0_coset_evals: Vec<E::Fr>,
    pub(crate) caulk_plus_precomputed: Precomputed<E>,
}

impl<E: PairingEngine> ProverPrecomputedData<E> {
    pub fn index(
        pk: &ProvingKey<E>,
        mimc_round_constants: &Vec<E::Fr>,
        indices: &[usize],
        c: &DensePolynomial<E::Fr>,
        table_size: usize,
    ) -> Self {
        let domain = GeneralEvaluationDomain::<E::Fr>::new(SUBGROUP_SIZE).unwrap();
        let extended_coset_domain =
            GeneralEvaluationDomain::<E::Fr>::new(EXTENDED_DOMAIN_FACTOR * SUBGROUP_SIZE).unwrap();

        // Compute zh inverse coset evals
        let mut zh_inverse_coset_evals =
            compute_vanishing_poly_over_coset(extended_coset_domain, domain.size() as u64);
        ark_ff::batch_inversion(&mut zh_inverse_coset_evals);

        // Compute mimc_cts coset evals
        assert_eq!(mimc_round_constants.len(), NUMBER_OF_MIMC_ROUNDS);
        let mut mimc_cts_evals = mimc_round_constants[..].to_vec();
        let mut to_append: Vec<E::Fr> = iter::repeat(E::Fr::from(DUMMY_VALUE))
            .take(SUBGROUP_SIZE - mimc_cts_evals.len())
            .collect();
        mimc_cts_evals.append(&mut to_append);

        // A polynomial over the multiplicative subgroup which evaluates to the MiMC7 round
        // constants at each root of unity. The subgroup size is the number of MiMC7 rounds.
        let mimc_cts_poly = DensePolynomial::from_coefficients_slice(&domain.ifft(&mimc_cts_evals));

        // We first compute a polynomial which evaluates, at each root of unity in the subgroup
        // domain, to a vector (of the size of the subgroup) consisting of the evaluations of the
        // MiMC7 round constants, padded by dummy values. Next, we perform an FFT over the coset of
        // the extended domain on the coefficients of this polynomial to obtain
        // \\(\mathsf{mimc\\_cts\\_coset\\_evals}\\).
        let mimc_cts_coset_evals = extended_coset_domain.coset_fft(&mimc_cts_poly);

        // Compute q_mimc coset evals
        let mut q_mimc_evals: Vec<E::Fr> = iter::repeat(E::Fr::one())
            .take(NUMBER_OF_MIMC_ROUNDS)
            .collect();
        let mut zeroes: Vec<E::Fr> = iter::repeat(E::Fr::zero())
            .take(SUBGROUP_SIZE - NUMBER_OF_MIMC_ROUNDS)
            .collect();
        q_mimc_evals.append(&mut zeroes);

        // A polynomial whose evaluations at the roots of unity over the subgroup domain of size
        // 128 are 91 `1` values, followed by zeroes. It represents the q_mimc selector column.
        let q_mimc = DensePolynomial::from_coefficients_slice(&domain.ifft(&q_mimc_evals));
        let q_mimc_coset_evals = extended_coset_domain.coset_fft(&q_mimc);

        // Compute l0 coset evals
        let mut l0_evals = vec![E::Fr::zero(); domain.size()];
        l0_evals[0] = E::Fr::one();

        let l0 = DensePolynomial::from_coefficients_slice(&domain.ifft(&l0_evals));
        let l0_coset_evals = extended_coset_domain.coset_fft(&l0);

        // Precompute w1 & w2 for the Caulk+ part of the proof
        let domain_t = GeneralEvaluationDomain::new(table_size).unwrap();
        let mut precomputed = Precomputed::<E>::empty();

        // As defined in the [Caulk+ paper, section 3](https://eprint.iacr.org/2022/957.pdf).
        precomputed.precompute_w1(&pk.srs_g2, indices, c, &domain_t);
        precomputed.precompute_w2(&pk.srs_g2, indices, &domain_t);

        Self {
            mimc_cts: mimc_cts_poly,
            mimc_cts_coset_evals,
            zh_inverse_coset_evals,
            q_mimc,
            q_mimc_coset_evals,
            l0_coset_evals,
            caulk_plus_precomputed: precomputed,
        }
    }
}

pub struct ProvingKey<E: PairingEngine> {
    pub srs_g1: Vec<E::G1Affine>,
    pub srs_g2: Vec<E::G2Affine>,
}

#[derive(CanonicalSerialize, CanonicalDeserialize, Clone, Debug)]
pub struct PublicData<E: PairingEngine> {
    pub accumulator: E::G1Affine,
    pub external_nullifier: E::Fr,
    pub signal_hash: E::Fr,
    pub nullifier_hash: E::Fr,
}
