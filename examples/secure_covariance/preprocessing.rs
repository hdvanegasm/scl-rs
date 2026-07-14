use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::PartyId,
    prelude::{Error, Protocol, ProtocolId, RandEnvironment},
    protocol::passive_shamir::{
        double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr, triple::PassiveTriple,
    },
};

use crate::{Triple, THRESHOLD, VECTOR_LEN};

/// The offline phase: generate the `VECTOR_LEN` multiplication triples the online phase will spend.
///
/// Nothing here depends on the inputs, which is the whole point — this could have run last night.
/// Each pass produces `n - t` triples (DN07 extracts `n - t` random sharings from `n` dealt ones),
/// so two passes cover the six products.
pub struct Preprocessing {
    pub parties: Vec<PartyId>,
    pub king: PartyId,
}

#[async_trait::async_trait]
impl<E: RandEnvironment> Protocol<E> for Preprocessing {
    type Output = Vec<Triple>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let mut triples: Vec<Triple> = Vec::with_capacity(VECTOR_LEN);

        while triples.len() < VECTOR_LEN {
            // Random sharings of values no party knows: these become the triples' `a` and `b`.
            let a = PassiveRandShr::<1, Mersenne61>::new(THRESHOLD, self.parties.clone())?
                .execute(env)
                .await?;
            let b = PassiveRandShr::<1, Mersenne61>::new(THRESHOLD, self.parties.clone())?
                .execute(env)
                .await?;
            // The correlated randomness that pulls the degree-2t product back down to degree t.
            let doubles =
                PassiveRandDoubleShr::<1, Mersenne61>::new(THRESHOLD, self.parties.clone())?
                    .execute(env)
                    .await?;

            let batch = PassiveTriple::new(self.king, self.parties.clone(), a, b, doubles)?
                .execute(env)
                .await?;
            triples.extend(batch);
        }

        triples.truncate(VECTOR_LEN);
        Ok(triples)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Preprocessing")
    }
}
