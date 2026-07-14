use crate::{
    abbreviate::Abbreviate,
    math::{field::FiniteField, vector::Vector},
    net::PartyId,
    protocol::{
        passive_shamir::open_king::BatchedPassiveOpenToKing, Error, Protocol, ProtocolId,
        RandEnvironment,
    },
    ss::shamir::{DoubleShare, ShamirSS},
};

/// A multiplication triple: sharings of `a`, `b` and `a · b`, all of degree `t`.
pub struct ShamirTriple<const LIMBS: usize, F> {
    a: ShamirSS<LIMBS, F>,
    b: ShamirSS<LIMBS, F>,
    mult: ShamirSS<LIMBS, F>,
}

impl<const LIMBS: usize, F> ShamirTriple<LIMBS, F> {
    /// Assembles a triple from sharings of `a`, `b` and their product.
    pub fn new(a: ShamirSS<LIMBS, F>, b: ShamirSS<LIMBS, F>, mult: ShamirSS<LIMBS, F>) -> Self {
        Self { a, b, mult }
    }

    /// Returns the sharing of `a`.
    pub fn a(&self) -> &ShamirSS<LIMBS, F> {
        &self.a
    }

    /// Returns the sharing of `b`.
    pub fn b(&self) -> &ShamirSS<LIMBS, F> {
        &self.b
    }

    /// Returns the sharing of the product `a · b`.
    pub fn mult(&self) -> &ShamirSS<LIMBS, F> {
        &self.mult
    }

    /// Splits the triple into the sharings of `a`, `b` and `a · b`.
    pub fn into_parts(self) -> (ShamirSS<LIMBS, F>, ShamirSS<LIMBS, F>, ShamirSS<LIMBS, F>) {
        (self.a, self.b, self.mult)
    }
}

/// DN07 triple generation: turns random sharings `[a]`, `[b]` and the correlated randomness of
/// [`PassiveRandDoubleShr`](super::double_rand_share::PassiveRandDoubleShr) into multiplication
/// triples `([a], [b], [a · b])`.
///
/// For each triple the parties multiply their shares locally, which yields a degree-`2t` sharing of
/// `a · b` whose polynomial is *not* uniformly random. Masking it with the degree-`2t` half of a
/// double sharing makes it uniform, so `d = a · b + r` can be safely opened; subtracting the
/// degree-`t` half of the same double sharing then gives `[a · b]` back at degree `t`. All `d`
/// values are opened in a **single** batched round (see
/// [`super::open_king::BatchedPassiveOpenToKing`]), which is what keeps
/// the round count independent of how many triples are produced.
pub struct PassiveTriple<const LIMBS: usize, F> {
    a_shares: Vec<ShamirSS<LIMBS, F>>,
    b_shares: Vec<ShamirSS<LIMBS, F>>,
    double_shares: Vec<DoubleShare<LIMBS, F>>,
    parties: Vec<PartyId>,
    king: PartyId,
}

impl<const LIMBS: usize, F> PassiveTriple<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    /// Creates the protocol producing one triple per element of the three input vectors.
    ///
    /// The `i`-th triple consumes `a_shares[i]`, `b_shares[i]` and `double_shares[i]`, so the three
    /// vectors are positional and must have the same length. `king` is the party that collects the
    /// masked products and reconstructs them; every party must pass the same `king` and the same
    /// `parties`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`] if the three input vectors differ in length or are empty, if `king`
    /// is not one of `parties`, if the degrees of `a_shares[i]`, `b_shares[i]` and
    /// `double_shares[i]` disagree, or if `2 * t >= parties.len()` — the masked product is a
    /// degree-`2t` sharing, so fewer than `2t + 1` parties could not open it.
    pub fn new(
        king: PartyId,
        mut parties: Vec<PartyId>,
        a_shares: Vec<ShamirSS<LIMBS, F>>,
        b_shares: Vec<ShamirSS<LIMBS, F>>,
        double_shares: Vec<DoubleShare<LIMBS, F>>,
    ) -> Result<Self, Error> {
        let n_triples = a_shares.len();
        if n_triples == 0
            || b_shares.len() != n_triples
            || double_shares.len() != n_triples
            || !parties.contains(&king)
        {
            return Err(Error::Input);
        }

        for ((a, b), double) in a_shares.iter().zip(&b_shares).zip(&double_shares) {
            let t = double.degree();
            if a.degree() != t || b.degree() != t || 2 * t >= parties.len() {
                return Err(Error::Input);
            }
        }

        parties.sort();
        Ok(Self {
            a_shares,
            b_shares,
            double_shares,
            parties,
            king,
        })
    }
}

#[async_trait::async_trait]
impl<const LIMBS: usize, F, E> Protocol<E> for PassiveTriple<LIMBS, F>
where
    E: RandEnvironment,
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + Abbreviate + 'static,
{
    type Output = Vec<ShamirTriple<LIMBS, F>>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let mut d_vec = Vec::new();
        for ((a, b), (_, r_2t)) in self
            .a_shares
            .iter()
            .zip(self.b_shares.clone())
            .zip(self.double_shares.iter().map(|d| d.parts()))
        {
            let d_share_2t = (a.clone() * &b) + r_2t;
            d_vec.push(d_share_2t);
        }
        let open_protocol = BatchedPassiveOpenToKing::new(self.king, self.parties.clone(), d_vec);
        let d = open_protocol.execute(env).await?;

        let mut r_t_vec = Vec::new();
        for (r_t, _) in self.double_shares.iter().map(|d| d.parts()) {
            r_t_vec.push(r_t.clone());
        }
        let c_shares_vec = Vector::from(d)
            .add_shares(-Vector::from(r_t_vec))
            .map_err(|e| Error::Share(Box::new(e)))?;

        let mut result = Vec::new();
        for (a, (b, c)) in self
            .a_shares
            .into_iter()
            .zip(self.b_shares.into_iter().zip(c_shares_vec))
        {
            result.push(ShamirTriple::new(a, b, c));
        }
        Ok(result)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveShamirTriple")
    }
}
