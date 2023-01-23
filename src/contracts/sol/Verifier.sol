// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import { Types } from "./Types.sol";
import { Constants } from "./Constants.sol";
import { TranscriptLibrary } from "./Transcript.sol";
import { Lagrange } from "./Lagrange.sol";

contract Verifier {
    function verify(
        Types.Proof memory proof,
        uint256 externalNullifier,
        uint256 nullifierHash
    ) public view returns (
        uint256 debug0,
        uint256 debug1
    ) {
        uint256 p = Constants.PRIME_R;
        TranscriptLibrary.Transcript memory transcript = TranscriptLibrary.newTranscript();
        Types.ChallengeTranscript memory challengeTranscript;
        Types.VerifierTranscript memory verifierTranscript;

        TranscriptLibrary.updateWithG1(transcript, proof.commitments.w0);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.key);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.w1);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.w2);

        challengeTranscript.v = TranscriptLibrary.getChallenge(transcript);

        TranscriptLibrary.updateWithG1(transcript, proof.commitments.quotient);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.zi);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.ci);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.u_prime);

        TranscriptLibrary.getChallenge(transcript);
        challengeTranscript.hi_2 = TranscriptLibrary.getChallenge(transcript);

        TranscriptLibrary.updateWithG2(transcript, proof.commitments.w);
        TranscriptLibrary.updateWithG1(transcript, proof.commitments.h);

        challengeTranscript.alpha = TranscriptLibrary.getChallenge(transcript);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.w0_0);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w0_1);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w0_2);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.w1_0);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w1_1);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w1_2);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.w2_0);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w2_1);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.w2_2);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.key_0);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.key_1);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.q_mimc);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.c);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.quotient);

        TranscriptLibrary.updateWithU256(transcript, proof.openings.u_prime);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.p1);
        TranscriptLibrary.updateWithU256(transcript, proof.openings.p2);

        challengeTranscript.x1 = TranscriptLibrary.getChallenge(transcript);
        challengeTranscript.x2 = TranscriptLibrary.getChallenge(transcript);

        TranscriptLibrary.updateWithG1(transcript, proof.multiopenProof.f_cm);

        challengeTranscript.x3 = TranscriptLibrary.getChallenge(transcript);
        challengeTranscript.x4 = TranscriptLibrary.getChallenge(transcript);
        uint256[8] memory inverted;
        
        {
             // Values needed before batch inversion:
             // - d (so we can invert d - 1)
             // - x3_challenge
             // - proof.openings.u_prime_opening
             // - alpha_challenge
             // - omega_alpha
             // - omega_n_alpha

             // Values to invert:
             // - (d - 1) for the l0_eval computation
             // - xi_minus_v = x3_challenge - proof.openings.u_prime_opening
             // - xi_minus_alpha = x3_challenge - alpha_challenge
             // - xi_minus_omega_alpha = x3_challenge - omega_alpha;
             // - xi_minus_omega_n_alpha = x3_challenge - omega_n_alpha
             // - alpha_minus_omega_alpha = alpha - omega_alpha;
             // - alpha_minus_omega_n_alpha = alpha - omega_n_alpha;
             // - omega_alpha_minus_omega_n_alpha = omega_alpha - omega_n_alpha;

            // Compute and store omega_alpha, omega_n_alpha
            uint256 omega_alpha = Constants.omega;
            uint256 omega_n_alpha = Constants.omega_n;
            assembly {
                let alpha := mload(add(challengeTranscript, 0x40))
                omega_alpha := mulmod(omega_alpha, alpha, p)
                omega_n_alpha := mulmod(omega_n_alpha, alpha, p)
            }

            // Compute d - 1
            if (challengeTranscript.alpha == 0) {
                inverted[0] = p - 1;
            } else {
                inverted[0] = challengeTranscript.alpha - 1;
            }

            // Compute inputs to the batch inversion function
            assembly {
                let alpha := mload(add(challengeTranscript, 0x40))
                let x3 := mload(add(challengeTranscript, 0xa0))
                let v := mload(add(proof, 0xa0))
                let u_prime_opening := mload(add(mload(add(proof, 0x20)), 0x60))
                let xi_minus_v := addmod(x3, sub(p, u_prime_opening), p)
                let xi_minus_alpha := addmod(x3, sub(p, alpha), p)
                let xi_minus_omega_alpha := addmod(x3, sub(p, omega_alpha), p)
                let xi_minus_omega_n_alpha := addmod(x3, sub(p, omega_n_alpha), p)
                let alpha_minus_omega_alpha := addmod(alpha, sub(p, omega_alpha), p)
                let alpha_minus_omega_n_alpha := addmod(alpha, sub(p, omega_n_alpha), p)
                let omega_alpha_minus_omega_n_alpha := addmod(omega_alpha, sub(p, omega_n_alpha), p)

                /* 0    (d - 1) is already stored */
                /* 1 */ mstore(add(inverted, 0x20), xi_minus_v)
                /* 2 */ mstore(add(inverted, 0x40), xi_minus_alpha)
                /* 3 */ mstore(add(inverted, 0x60), xi_minus_omega_alpha)
                /* 4 */ mstore(add(inverted, 0x80), xi_minus_omega_n_alpha)
                /* 5 */ mstore(add(inverted, 0xa0), alpha_minus_omega_alpha)
                /* 6 */ mstore(add(inverted, 0xc0), alpha_minus_omega_n_alpha)
                /* 7 */ mstore(add(inverted, 0xe0), omega_alpha_minus_omega_n_alpha)
            }
        }

        {
        inverted = batchInvert(inverted);

        (uint256 l0Eval, uint256 zhEval) = Lagrange.computeL0AndVanishingEval(
            challengeTranscript.alpha,
            inverted[0]
        );

        assembly {
            // Store the inverted values to verifierTranscript. They will be
            // used in the multiopen veriifer step
            mstore(add(verifierTranscript, 0x60), mload(inverted))
            mstore(add(verifierTranscript, 0x80), mload(add(inverted, 0x20)))
            mstore(add(verifierTranscript, 0xa0), mload(add(inverted, 0x40)))
            mstore(add(verifierTranscript, 0xc0), mload(add(inverted, 0x60)))
            mstore(add(verifierTranscript, 0xe0), mload(add(inverted, 0x80)))
            mstore(add(verifierTranscript, 0x100), mload(add(inverted, 0xa0)))
            mstore(add(verifierTranscript, 0x120), mload(add(inverted, 0xc0)))
            mstore(add(verifierTranscript, 0x140), mload(add(inverted, 0xe0)))

            // Store l0Eval, zhEval in verifierTranscript. They will be used in
            // verifyGateEvals()
            mstore(add(verifierTranscript, 0x160), l0Eval)
            mstore(add(verifierTranscript, 0x180), zhEval)
        }

        require(
            verifyGateEvals(
                proof,
                verifierTranscript,
                challengeTranscript.v,
                nullifierHash,
                externalNullifier
            ),
            "Verifier: gate check failed"
        );
        }
    }

    function verifyGateEvals(
        Types.Proof memory proof,
        Types.VerifierTranscript memory verifierTranscript,
        uint256 v_challenge,
        uint256 nullifierHash,
        uint256 externalNullifier
    ) public pure returns (bool) {
        uint256 p = Constants.PRIME_R;
        uint256 rhs;
        uint256 lhs;

        assembly {
            function pow7(val, prime) -> r {
                let val2 := mulmod(val, val, prime)
                let val4 := mulmod(val2, val2, prime)
                let val6 := mulmod(val2, val4, prime)
                r := mulmod(val6, val, prime)
            }

            let rolling_v := v_challenge
            let openingsPtr := mload(add(proof, 0x20))

            {
            // Compute rhs = zh_eval * quotient_opening
            let zh_eval := mload(add(verifierTranscript, 0x180))
            let quotient_opening := mload(add(openingsPtr, 0x40))
            rhs := mulmod(zh_eval, quotient_opening, p)
            }

            {
            //let gate_0_eval = q_mimc_opening * (pow_7(w0_openings[0] + c_opening) - w0_openings[1]);
            let q_mimc := mload(openingsPtr)
            let c := mload(add(openingsPtr, 0x20))
            let w0_0 := mload(add(openingsPtr, 0xc0))
            let w0_1 := mload(add(openingsPtr, 0xe0))

            let gate_0_eval := addmod(w0_0, c, p)
            gate_0_eval := pow7(gate_0_eval, p)
            gate_0_eval := addmod(gate_0_eval, sub(p, w0_1), p)
            gate_0_eval := mulmod(gate_0_eval, q_mimc, p)
            lhs := gate_0_eval
            }

            {
            let q_mimc := mload(openingsPtr)
            let c := mload(add(openingsPtr, 0x20))
            // Gate 1: q_mimc_opening * ((w1_openings[0] + key_openings[0] + c_opening) ^ 7 - w1_openings[1])
            let w1_0 := mload(add(openingsPtr, 0x120))
            let w1_1 := mload(add(openingsPtr, 0x140))
            let key_0 := mload(add(openingsPtr, 0x1e0))
            let gate_1_eval := addmod(addmod(w1_0, key_0, p), c, p)
            gate_1_eval := pow7(gate_1_eval, p)
            gate_1_eval := addmod(gate_1_eval, sub(p, w1_1), p)
            gate_1_eval := mulmod(gate_1_eval, q_mimc, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_1_eval, p), p)

            rolling_v := mulmod(rolling_v, v_challenge, p)
            }

            {
            // Gate 2: q_mimc_opening * ((w2_openings[0] + key_openings[0] + c_opening) ^ 7 - w2_openings[1]) 
            let q_mimc := mload(openingsPtr)
            let c := mload(add(openingsPtr, 0x20))
            let key_0 := mload(add(openingsPtr, 0x1e0))
            let w2_0 := mload(add(openingsPtr, 0x180))
            let w2_1 := mload(add(openingsPtr, 0x1a0))
            let gate_2_eval := addmod(addmod(w2_0, key_0, p), c, p)
            gate_2_eval := pow7(gate_2_eval, p)
            gate_2_eval := addmod(gate_2_eval, sub(p, w2_1), p)
            gate_2_eval := mulmod(gate_2_eval, q_mimc, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_2_eval, p), p)

            rolling_v := mulmod(rolling_v, v_challenge, p)
            }

            {
            // Gate 3: q_mimc_opening * (key_openings[0] - key_openings[1])
            let q_mimc := mload(openingsPtr)
            let key_0 := mload(add(openingsPtr, 0x1e0))
            let key_1 := mload(add(openingsPtr, 0x200))
            let gate_3_eval := addmod(key_0, sub(p, key_1), p)
            gate_3_eval := mulmod(gate_3_eval, q_mimc, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_3_eval, p), p)

            rolling_v := mulmod(rolling_v, v_challenge, p)

            // Gate 4: l0 * (key_openings[0] - w0_openings[0] - w0_openings[2])
            let w0_0 := mload(add(openingsPtr, 0xc0))
            let w0_2 := mload(add(openingsPtr, 0x100))
            let l0 := mload(add(verifierTranscript, 0x160))
            let gate_4_eval := addmod(key_0, sub(p, addmod(w0_0, w0_2, p)), p)
            gate_4_eval := mulmod(gate_4_eval, l0, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_4_eval, p), p)

            rolling_v := mulmod(rolling_v, v_challenge, p)

            // Gate 5: l0 * (nullifierHash - w2_openings[0] - w2_openings[2] - (2 * key_openings[0])) 
            let w2_0 := mload(add(openingsPtr, 0x180))
            let w2_2 := mload(add(openingsPtr, 0x1c0))
            let two_key_0 := addmod(key_0, key_0, p)
            let r := addmod(w2_0, w2_2, p)
            r := addmod(r, two_key_0, p)
            let gate_5_eval := addmod(nullifierHash, sub(p, r), p)
            gate_5_eval := mulmod(gate_5_eval, l0, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_5_eval, p), p)

            rolling_v := mulmod(rolling_v, v_challenge, p)

            // Gate 6: l0 * (w2_openings[0] - external_nullifier)
            let gate_6_eval := addmod(w2_0, sub(p, externalNullifier), p)
            gate_6_eval := mulmod(gate_6_eval, l0, p)
            lhs := addmod(lhs, mulmod(rolling_v, gate_6_eval, p), p)
            }
        }
        return lhs == rhs;
    }

    function batchInvert(
        uint256[8] memory inputs
    ) public view returns (uint256[8] memory) {
        uint256[8] memory results;
        uint256 p = Constants.PRIME_R;
        assembly {
            let mPtr := mload(0x40)
            /*
               0x0   b_1 = inputs[1] * inputs[0]
               0x20  b_2 = inputs[2] * b_1
               0x40  b_3 = inputs[3] * b_2
               0x60  b_4 = inputs[4] * b_3
               0x80  b_5 = inputs[5] * b_4
               0xa0  b_6 = inputs[6] * b_5
               0xc0  b_7 = inputs[7] * b_6
               0xe0      = input to modexp precompile
               0x100     = input to modexp precompile
               0x120     = input to modexp precompile
               0x140     = input to modexp precompile
               0x160     = input to modexp precompile
               0x180     = input to modexp precompile
               0x1a0 t_0 = t_1 * inputs[1] (output)
               0x1c0 t_1 = t_2 * inputs[2]
               0x1e0 t_2 = t_3 * inputs[3]
               0x200 t_3 = t_4 * inputs[4]
               0x220 t_4 = t_5 * inputs[5]
               0x240 t_5 = t_6 * inputs[6]
               0x260 t_6 = t_7 * inputs[7]
               0x280 t_7 = inverse(b_7)
               0x2a0 c_1 = t_1 * b_0 (output)
               0x2c0 c_2 = t_2 * b_1 (output)
               0x2e0 c_3 = t_3 * b_2 (output)
               0x300 c_4 = t_4 * b_3 (output)
               0x320 c_5 = t_5 * b_4 (output)
               0x340 c_6 = t_6 * b_5 (output)
               0x360 c_7 = t_7 * b_6 (output)

               Output t_0, c_1, ..., c_7
             */

            // 1. Compute and store b values
            let a_0 := mload(inputs)
            let a_1 := mload(add(inputs, 0x20))
            let b_1 := mulmod(a_0, a_1, p)
            // Store b_1
            mstore(mPtr, b_1)

            for { let i := 1 } lt(i, 8) { i := add(i, 1) } {
                let offset := mul(i, 0x20)
                let a_i := mload(add(inputs, add(offset, 0x20)))
                let b_i_minus_1 := mload(add(mPtr, sub(offset, 0x20)))
                let b_i := mulmod(a_i, b_i_minus_1, p)
                mstore(add(mPtr, offset), b_i)
            }

            // Revert if any of the inputs are 0 (which will cause b_n to be 0)
            switch mload(add(mPtr, 0xc0)) case 0 { revert(0, 0) }


            // 2. Compute and store t_7
            mstore(add(mPtr, 0x0e0), 0x20)
            mstore(add(mPtr, 0x100), 0x20)
            mstore(add(mPtr, 0x120), 0x20)
            mstore(add(mPtr, 0x140), mload(add(mPtr, 0xc0)))
            mstore(add(mPtr, 0x160), sub(p, 2))
            mstore(add(mPtr, 0x180), p)
            let success := staticcall(gas(), 0x05, add(mPtr, 0x0e0), 0xc0, add(mPtr, 0x280), 0x20)
            switch success case 0 { revert(0, 0) }

            // 3. Compute and store t_6, .., t_0
            for { let index := 0 } lt(index, 8) { index := add(index, 1) } {
                let i := sub(7, index)
                let a_i := mload(add(inputs, mul(i, 0x20)))
                let offset := add(0x1a0, mul(i, 0x20))
                let t_i_plus_1 := mload(add(mPtr, offset))
                let t_i := mulmod(a_i, t_i_plus_1, p)
                mstore(add(mPtr, sub(offset, 0x20)), t_i)
            }

            // 6. Compute and store c_1
            let c_1 := mulmod(
                mload(add(mPtr, 0x1c0)),
                mload(inputs),
                p
            )
            mstore(add(mPtr, 0x2a0), c_1)

            // 5. Compute and store c_2, ..., c_7
            for { let i := 2 } lt(i, 8) { i := add(i, 1) } {
                let offst := mul(i, 0x20)
                let t_offst := add(0x1a0, offst)
                let b_offst := mul(sub(i, 2), 0x20)

                let t_i := mload(add(mPtr, t_offst))
                let b_i_minus_1 := mload(add(mPtr, b_offst))

                let c_i := mulmod(t_i, b_i_minus_1, p)

                mstore(add(mPtr, add(offst, 0x280)), c_i)
            }

            mstore(    results,        mload(add(mPtr, 0x01a0))) // t0
            mstore(add(results, 0x20), mload(add(mPtr, 0x02a0))) // c1
            mstore(add(results, 0x40), mload(add(mPtr, 0x02c0))) // c2
            mstore(add(results, 0x60), mload(add(mPtr, 0x02e0))) // c3
            mstore(add(results, 0x80), mload(add(mPtr, 0x0300))) // c4
            mstore(add(results, 0xa0), mload(add(mPtr, 0x0320))) // c5
            mstore(add(results, 0xc0), mload(add(mPtr, 0x0340))) // c6
            mstore(add(results, 0xe0), mload(add(mPtr, 0x0360))) // c7
        }

        return results;
    }
}