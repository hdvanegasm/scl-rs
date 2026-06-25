//! Display-only type labels for elements carried in a [`Packet`](crate::net::Packet).
//!
//! When a protocol writes a typed value into a packet with
//! [`Packet::write_labeled`](crate::net::Packet::write_labeled), the value's type supplies a short,
//! human-readable label through the [`Abbreviate`] trait. The deterministic simulator aggregates
//! these labels per packet (see [`Packet::composition`](crate::net::Packet::composition)) and shows
//! them in the `SEND` line of an event trace, so a trace records *what kind* of data crossed the
//! wire, not just how many bytes:
//!
//! ```text
//! SEND    2 -> 0 (1024 bytes: 1 EC elem., 2 Shamir shr., 4 field elem.)
//! ```
//!
//! The label is metadata for local tracing only: it is never serialized onto the wire, so it costs
//! no bandwidth and does not affect packet equality. Types written through the plain
//! [`Packet::write`](crate::net::Packet::write) (or any type that does not implement `Abbreviate`)
//! are reported as `unknown elem.`.

/// A short, display-only label for an element type, used to enrich network traces.
///
/// Implement this on the concrete types a protocol sends (field elements, curve points, shares,
/// …) so that [`Packet::write_labeled`](crate::net::Packet::write_labeled) can record what kind of
/// element each packet entry is. The label is a property of the *type*, not of a particular value,
/// so it is an associated constant rather than a method.
///
/// # Example
///
/// ```
/// use scl_rs::abbreviate::Abbreviate;
///
/// struct PublicKey;
///
/// impl Abbreviate for PublicKey {
///     const ABBREVIATION: &'static str = "pub. key";
/// }
///
/// assert_eq!(PublicKey::ABBREVIATION, "pub. key");
/// ```
pub trait Abbreviate {
    /// The abbreviated label shown for this element type in a trace (e.g. `"field elem."`).
    ///
    /// Keep it short — it is repeated inline in the `SEND` event line and aggregated by value, so
    /// every value of the same type must report the same string for the per-type counts to add up.
    const ABBREVIATION: &'static str;
}
