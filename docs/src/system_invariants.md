# System invariants

An [invariant](https://mathworld.wolfram.com/Invariant.html) is a property of a
system which remains unmodified even after operations or transformations are
applied to it. The authors of Semacaulk intend the following to be the
invariants of Semacaulk:

1. **Privacy**: No-one but the user who knows the value of the identity nullifier and
   identity trapdoor behind an identity commitment may generate a valid proof
   of set membership of the identity commitment in the accumulator.

2. **Safe NUMS value**: No-one should be able to produce a valid proof of set
   membership for an element of the set that is set to the default
   nothing-up-my-sleeve value.

3. **Proof non-malleability**: Proofs are visible once submitted to the mempool,
   but no-one should be able to modify an existing proof, change it such that
   it is associated with a different signal, and remain valid.

4. **Zero-knowledge**: given a valid proof, no-one should be able to determine
   the index of the identity commitment of which the prover knows the preimage.

5. 

TODO: add more invariants