use crate::{
    abbreviate::Abbreviate,
    math::field::FiniteField,
    net::PartyId,
    protocol::{
        passive_shamir::{open_king::BatchedPassiveOpenToKing, triple::ShamirTriple},
        Error, Protocol, ProtocolId, RandEnvironment,
    },
    ss::shamir::ShamirSS,
};

/// Beaver multiplication: turns sharings `[x]` and `[y]` into a sharing of their product, by
/// consuming one multiplication triple per product.
///
/// A local product of two degree-`t` sharings lands at degree `2t`, so it cannot be used as an
/// input to the next multiplication. The Beaver trick avoids the problem entirely: with a triple
/// `([a], [b], [a · b])` the parties open the two **masked** values `d = x − a` and `e = y − b`,
/// which reveal nothing because `a` and `b` are uniformly random and unknown, and then recover the
/// product from public constants and linear operations alone:
///
/// ```text
/// [x · y] = [a · b] + d · [b] + e · [a] + d · e
/// ```
///
/// Everything on the right is either a share scaled by a public constant or a public constant
/// added to a share, so the result comes straight back out at degree `t` with no degree reduction
/// needed.
///
/// The whole batch costs **one** round: all `2ℓ` masked values are opened together through a
/// single [`BatchedPassiveOpenToKing`]. The round count of a circuit therefore tracks its
/// multiplicative *depth*, not its number of multiplication gates — multiply everything that sits
/// at the same depth in one `PassiveShamirMul`.
///
/// The triples come from [`PassiveTriple`](super::triple::PassiveTriple) and are **consumed**: a
/// triple reused for a second product would mask it with the same `a` and `b`, and the two openings
/// together would reveal both inputs.
pub struct PassiveShamirMul<const LIMBS: usize, F> {
    x_shares: Vec<ShamirSS<LIMBS, F>>,
    y_shares: Vec<ShamirSS<LIMBS, F>>,
    triples: Vec<ShamirTriple<LIMBS, F>>,
    parties: Vec<PartyId>,
    king: PartyId,
}

impl<const LIMBS: usize, F> PassiveShamirMul<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    /// Creates the protocol computing one product per element of the input vectors: the `i`-th
    /// output is a sharing of `x_shares[i] · y_shares[i]`, consuming `triples[i]`.
    ///
    /// The three vectors are positional and must have the same length. `king` is the party that
    /// reconstructs the masked values; every party must pass the same `king` and `parties`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`] if the three vectors differ in length or are empty, if `king` is
    /// not one of `parties`, if the shares and their triple do not all have the same degree `t`, or
    /// if `t >= parties.len()`, which would leave the masked values unopenable. Note that unlike
    /// triple *generation*, this protocol opens only degree-`t` sharings, so it does not need
    /// `n >= 2t + 1`.
    pub fn new(
        king: PartyId,
        mut parties: Vec<PartyId>,
        x_shares: Vec<ShamirSS<LIMBS, F>>,
        y_shares: Vec<ShamirSS<LIMBS, F>>,
        triples: Vec<ShamirTriple<LIMBS, F>>,
    ) -> Result<Self, Error> {
        let n_products = x_shares.len();
        if n_products == 0
            || y_shares.len() != n_products
            || triples.len() != n_products
            || !parties.contains(&king)
        {
            return Err(Error::Input);
        }

        for ((x, y), triple) in x_shares.iter().zip(&y_shares).zip(&triples) {
            let t = x.degree();
            if y.degree() != t
                || triple.a().degree() != t
                || triple.b().degree() != t
                || triple.mult().degree() != t
                || t >= parties.len()
            {
                return Err(Error::Input);
            }
        }

        parties.sort();
        Ok(Self {
            x_shares,
            y_shares,
            triples,
            parties,
            king,
        })
    }
}

#[async_trait::async_trait]
impl<const LIMBS: usize, E, F> Protocol<E> for PassiveShamirMul<LIMBS, F>
where
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + Abbreviate + 'static,
    E: RandEnvironment,
{
    type Output = Vec<ShamirSS<LIMBS, F>>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let n_products = self.x_shares.len();

        // Mask both operands of every product with its triple, and open all of them at once: the
        // batch costs a single round regardless of how many products it holds.
        let mut masked = Vec::with_capacity(2 * n_products);
        for ((x, y), triple) in self.x_shares.iter().zip(&self.y_shares).zip(&self.triples) {
            masked.push(x.clone() - triple.a());
            masked.push(y.clone() - triple.b());
        }

        let opened = BatchedPassiveOpenToKing::new(self.king, self.parties.clone(), masked)
            .execute(env)
            .await?;

        // [x · y] = [a · b] + d · [b] + e · [a] + d · e — all local from here.
        let mut products = Vec::with_capacity(n_products);
        for (i, triple) in self.triples.into_iter().enumerate() {
            let d = opened[2 * i];
            let e = opened[2 * i + 1];
            let (a, b, c) = triple.into_parts();
            products.push(c + &(b * &d) + &(a * &e) + &(d * &e));
        }
        Ok(products)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveShamirMul")
    }
}
