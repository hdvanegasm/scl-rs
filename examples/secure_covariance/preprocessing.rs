use scl_rs::{
    math::field::mersenne61::Mersenne61,
    net::PartyId,
    prelude::{Error, ForkEnvironment, Join, JoinAll, Protocol, ProtocolId, RandEnvironment},
    protocol::passive_shamir::{
        double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr, triple::PassiveTriple,
    },
};

use crate::{Triple, THRESHOLD, VECTOR_LEN};

/// The offline phase: generate the `VECTOR_LEN` multiplication triples the online phase will spend.
///
/// Nothing here depends on the inputs, which is the whole point — this could have run last night.
///
/// A triple needs a random sharing `a`, a random sharing `b` and a double sharing. One run of
/// `PassiveRandShr` or `PassiveRandDoubleShr` yields `n - t` of them (DN07 extracts `n - t` random
/// sharings from `n` dealt ones), and that number is fixed, so `VECTOR_LEN` of each takes several
/// runs. Those runs are independent of each other, so they all run at once: a [`Join`] of three
/// [`JoinAll`]s, one per kind of randomness. Triple generation, on the other hand, is vectorized:
/// one `PassiveTriple` turns all `VECTOR_LEN` triples in one round and one message per link, so it
/// runs once over everything rather than once per batch of randomness.
pub struct Preprocessing {
    pub parties: Vec<PartyId>,
    pub king: PartyId,
}

impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for Preprocessing {
    type Output = Vec<Triple>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        // How many runs of each randomness protocol cover `VECTOR_LEN` sharings.
        let runs = VECTOR_LEN.div_ceil(self.parties.len() - THRESHOLD);
        let random = || -> Result<Vec<_>, Error> {
            (0..runs)
                .map(|_| PassiveRandShr::<1, Mersenne61>::new(THRESHOLD, self.parties.clone()))
                .collect()
        };
        let doubles = (0..runs)
            .map(|_| PassiveRandDoubleShr::<1, Mersenne61>::new(THRESHOLD, self.parties.clone()))
            .collect::<Result<Vec<_>, Error>>()?;

        // Random sharings of values no party knows, which become the triples' `a` and `b`, and
        // the correlated randomness that pulls the degree-2t product back down to degree t.
        let (a, b, doubles) = Join((JoinAll(random()?), JoinAll(random()?), JoinAll(doubles)))
            .execute(env)
            .await?;

        PassiveTriple::new(
            self.king,
            self.parties,
            first(a, VECTOR_LEN),
            first(b, VECTOR_LEN),
            first(doubles, VECTOR_LEN),
        )?
        .execute(env)
        .await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Preprocessing")
    }
}

/// The first `n` items of the runs' outputs, taken in run order.
fn first<T>(runs: Vec<Vec<T>>, n: usize) -> Vec<T> {
    runs.into_iter().flatten().take(n).collect()
}
