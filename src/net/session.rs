use std::fmt::Display;

use sha2::{Digest, Sha256};

/// Domain-separation prefix hashed into every [`SessionId::digest`], so that a session digest can
/// never collide with a hash computed for another purpose over the same bytes.
const DIGEST_DOMAIN: &[u8] = b"scl-rs/session-id/v1";

/// Domain-separation prefix hashed into every [`derive_root_tag`] output.
const ROOT_TAG_DOMAIN: &[u8] = b"scl-rs/root-tag/v1";

/// Derives a 32-byte root tag (see [`SessionId::root`]) from public context that every party of an
/// execution knows: SHA-256 over a domain-separation prefix and `context`.
///
/// No communication is involved, so the parties agree on the tag exactly when they pass the same
/// `context`. The tag is unique per execution only if `context` is. Put in it whatever
/// distinguishes this execution from every other one on the same parties, typically:
///
/// - the protocol name and version;
/// - the party set;
/// - the parameters;
/// - an epoch or run counter that the application never reuses.
///
/// ```
/// use scl_rs::net::session::derive_root_tag;
///
/// let epoch: u64 = 17;
/// let mut context = b"my-protocol/v1 parties=0,1,2 t=1 epoch=".to_vec();
/// context.extend_from_slice(&epoch.to_le_bytes());
///
/// let tag = derive_root_tag(&context);
/// assert_eq!(tag, derive_root_tag(&context)); // every party computes the same tag
/// assert_ne!(tag, derive_root_tag(b"my-protocol/v1 parties=0,1,2 t=1 epoch=18"));
/// ```
pub fn derive_root_tag(context: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ROOT_TAG_DOMAIN);
    hasher.update(context);
    hasher.finalize().into()
}

/// Identifies a session: the scope in which a group of messages is exchanged, so that protocols
/// running concurrently over the same links never consume each other's messages.
///
/// A session has two parts.
///
/// - **A root tag**, 32 bytes that identify one *execution*. All parties of an execution must use
///   the same root tag, and different executions (two key generations, two epochs, two
///   deployments on the same parties) must use different ones. The tag is supplied from outside,
///   for example as a hash of the protocol name and version, the party set, the parameters, and an
///   epoch counter or fresh randomness agreed by all parties. This is what makes sessions distinct
///   *across* executions, in the sense the UC framework requires of session identifiers.
/// - **A path of fork indices** from the execution's root session. The root session is the empty
///   path. Forking a network handle creates a child session by appending the handle's next fork
///   index (see [`with_child`](SessionId::with_child)). Within an execution, sessions therefore
///   form a tree, and a session's ancestors are its path prefixes:
///
/// | Path        | Meaning                                             |
/// | ----------- | --------------------------------------------------- |
/// | `[]`        | the root session; all sequential code runs here     |
/// | `[1]`       | the second session forked from the root             |
/// | `[1, 0]`    | the first session forked from `[1]`                 |
///
/// Fork indices follow the order in which each party's program forks, with no communication
/// between parties. Two parties agree on a session's path only if they perform the same sequence
/// of forks within each session they share.
///
/// Equality, hashing and ordering compare the root tag and then the path, so a session rebuilt
/// from a received frame equals the one built locally with the same tag and path. Within one root
/// tag, ordering is lexicographic in the path, which is depth-first:
/// `[] < [0] < [1] < [1, 0] < [2]`.
///
/// # Routing and binding
///
/// The path is what routes messages. The root tag is fixed for a network instance, so a message
/// header needs to carry only the path (see [`path`](SessionId::path)).
///
/// [`digest`](SessionId::digest) commits to the root tag *and* the path. It is the value to hash
/// into anything that must not be replayed into another session: Fiat–Shamir transcripts,
/// commitments, signed messages, the statement of a proof. Nothing in the library binds it yet;
/// under the honest-but-curious model the session identifier is routing metadata only, and it is
/// not authenticated beyond what the channel authenticates.
///
/// # Example
///
/// ```
/// use scl_rs::net::session::SessionId;
///
/// let root = SessionId::root([7; 32]);
/// let first = root.with_child(0);
/// let nested = first.with_child(3);
///
/// assert_eq!(nested.path(), &[0, 3]);
/// assert_eq!(nested.depth(), 2);
/// assert_eq!(nested.parent(), Some(first));
/// assert_eq!(root.parent(), None);
///
/// // Same path under a different execution: a different session, with a different digest.
/// let other = SessionId::root([8; 32]).with_child(0).with_child(3);
/// assert_ne!(other, nested);
/// assert_ne!(other.digest(), nested.digest());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId {
    /// Identifies the execution; equal at every party of it, distinct across executions.
    root_tag: [u8; 32],
    /// Fork indices from the execution's root session; empty for the root session.
    path: Vec<u32>,
}

impl SessionId {
    /// Returns the root session of the execution identified by `root_tag`: the empty path.
    ///
    /// Every network handle of an execution starts in this session, and code that never forks
    /// runs entirely in it. All parties must pass the same `root_tag`.
    pub fn root(root_tag: [u8; 32]) -> Self {
        Self {
            root_tag,
            path: Vec::new(),
        }
    }

    /// Rebuilds a session from its execution's root tag and a path received in a message header.
    ///
    /// Crate-internal: outside the network backends, sessions are created only through
    /// [`root`](SessionId::root) and [`with_child`](SessionId::with_child).
    pub(crate) fn new(root_tag: [u8; 32], path: Vec<u32>) -> Self {
        Self { root_tag, path }
    }

    /// Returns the session forked from this one with fork index `index`: the same root tag, and
    /// this session's path with `index` appended.
    ///
    /// The fork index counts the forks made from this session, starting at 0: the first session
    /// forked from it has index 0, the second has index 1, and so on. Forking a network handle
    /// assigns these indices in that order (see [`ForkNetwork`](crate::net::ForkNetwork)).
    /// `with_child` only computes the resulting identifier: it does not fork anything, and does not
    /// check that such a fork happened.
    ///
    /// # Example
    ///
    /// Take the session `1.0`, with path `[1, 0]`: the first session forked from session `1`,
    /// which is itself the second session forked from the root. Each of its children extends that
    /// path by one index:
    ///
    /// ```
    /// use scl_rs::net::session::SessionId;
    ///
    /// let session = SessionId::root([7; 32]).with_child(1).with_child(0);
    /// assert_eq!(session.path(), &[1, 0]);
    ///
    /// // The first and the third sessions forked from `1.0`.
    /// assert_eq!(session.with_child(0).path(), &[1, 0, 0]);
    /// assert_eq!(session.with_child(2).path(), &[1, 0, 2]);
    /// assert_eq!(session.with_child(2).to_string(), "1.0.2");
    ///
    /// // A child keeps the root tag, and `session` itself is unchanged.
    /// assert_eq!(session.with_child(2).root_tag(), session.root_tag());
    /// assert_eq!(session.path(), &[1, 0]);
    /// ```
    pub fn with_child(&self, index: u32) -> Self {
        let mut curr_path = self.path.clone();
        curr_path.push(index);
        Self {
            root_tag: self.root_tag,
            path: curr_path,
        }
    }

    /// Returns the session this one was forked from: same root tag, this path without its last
    /// index. Returns `None` for the root session.
    ///
    /// For every session `s` and index `k`, `s.with_child(k).parent() == Some(s)`.
    pub fn parent(&self) -> Option<Self> {
        if self.path.is_empty() {
            None
        } else {
            Some(Self {
                root_tag: self.root_tag,
                path: self.path[..self.path.len() - 1].to_vec(),
            })
        }
    }

    /// Returns the number of forks between the root session and this one: `0` for the root
    /// session.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Returns the root tag of the execution this session belongs to.
    pub fn root_tag(&self) -> &[u8; 32] {
        &self.root_tag
    }

    /// Returns the fork indices from the root session: the part of the session a message header
    /// carries. The root tag is fixed per network instance, so it is not sent.
    pub fn path(&self) -> &[u32] {
        &self.path
    }

    /// Returns a 32-byte SHA-256 commitment to this session: its root tag and its path.
    ///
    /// The input is a domain-separation prefix, the root tag, the path length as a little-endian
    /// `u64`, and each index as a little-endian `u32`. The length prefix makes the encoding
    /// injective: distinct sessions hash distinct byte strings. So two sessions get equal digests
    /// only if they are equal, short of a SHA-256 collision.
    ///
    /// Hash this value into any transcript, commitment or signed message that must stay bound to
    /// its session.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(DIGEST_DOMAIN);
        hasher.update(self.root_tag);
        hasher.update((self.path.len() as u64).to_le_bytes());
        for index in &self.path {
            hasher.update(index.to_le_bytes());
        }
        hasher.finalize().into()
    }
}

/// Shows the path only: `root` for the root session, otherwise the fork indices joined by dots, e.g.
/// `0`, `1.0`, `0.3.1`.
///
/// The root tag is not shown, so the same path in two executions displays the same. Width, fill and
/// alignment apply as for a string: `format!("{:>6}", SessionId::root(tag))` is `"  root"`.
impl Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            return f.pad("root");
        }
        let path_str: Vec<_> = self.path.iter().map(|&e| e.to_string()).collect();
        f.pad(&path_str.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: [u8; 32] = [7; 32];

    /// Builds a session of execution `TAG` from a path literal.
    fn session(path: &[u32]) -> SessionId {
        SessionId::new(TAG, path.to_vec())
    }

    #[test]
    fn root_is_the_empty_path_of_its_execution() {
        let root = SessionId::root(TAG);
        assert_eq!(root, session(&[]));
        assert_eq!(root.depth(), 0);
        assert_eq!(root.path(), &[] as &[u32]);
        assert_eq!(root.root_tag(), &TAG);
    }

    #[test]
    fn child_appends_the_index_and_leaves_the_parent_unchanged() {
        let parent = session(&[1]);
        let child = parent.with_child(0);

        assert_eq!(child, session(&[1, 0]));
        assert_eq!(child.root_tag(), &TAG);
        assert_eq!(parent, session(&[1]));
    }

    #[test]
    fn distinct_indices_give_distinct_children() {
        let parent = session(&[2]);
        assert_ne!(parent.with_child(0), parent.with_child(1));
    }

    #[test]
    fn parent_inverts_child() {
        for path in [&[][..], &[0], &[1, 0], &[3, 1, 4, 1, 5]] {
            let s = session(path);
            for index in [0, 1, u32::MAX] {
                assert_eq!(s.with_child(index).parent(), Some(s.clone()));
            }
        }
    }

    #[test]
    fn root_has_no_parent() {
        assert_eq!(SessionId::root(TAG).parent(), None);
    }

    #[test]
    fn depth_counts_forks_from_the_root() {
        let s = SessionId::root(TAG)
            .with_child(1)
            .with_child(1)
            .with_child(0);
        assert_eq!(s, session(&[1, 1, 0]));
        assert_eq!(s.depth(), 3);
        assert_eq!(s.parent().unwrap().depth(), 2);
    }

    #[test]
    fn same_path_in_different_executions_is_a_different_session() {
        let here = SessionId::root([1; 32]).with_child(0);
        let there = SessionId::root([2; 32]).with_child(0);
        assert_eq!(here.path(), there.path());
        assert_ne!(here, there);
    }

    #[test]
    fn ordering_is_lexicographic_depth_first_within_an_execution() {
        let mut sessions = vec![
            session(&[2]),
            session(&[1, 0]),
            session(&[]),
            session(&[1]),
            session(&[0]),
            session(&[1, 1]),
        ];
        sessions.sort();
        assert_eq!(
            sessions,
            vec![
                session(&[]),
                session(&[0]),
                session(&[1]),
                session(&[1, 0]),
                session(&[1, 1]),
                session(&[2]),
            ]
        );
    }

    #[test]
    fn equal_sessions_hash_equally() {
        use std::collections::HashSet;

        let built = SessionId::root(TAG).with_child(1).with_child(0);
        let received = SessionId::new(TAG, vec![1, 0]);
        let set: HashSet<SessionId> = [built].into_iter().collect();
        assert!(set.contains(&received));
    }

    #[test]
    fn digest_is_deterministic() {
        assert_eq!(session(&[1, 0]).digest(), session(&[1, 0]).digest());
    }

    #[test]
    fn digest_separates_paths_and_executions() {
        let digests = [
            session(&[]).digest(),
            session(&[0]).digest(),
            session(&[0, 0]).digest(),
            session(&[1]).digest(),
            session(&[1, 0]).digest(),
            session(&[256]).digest(),
            SessionId::new([8; 32], vec![1, 0]).digest(),
        ];
        for (i, a) in digests.iter().enumerate() {
            for b in &digests[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn digest_matches_its_documented_encoding() {
        let s = session(&[1, 0]);
        let mut input = Vec::new();
        input.extend_from_slice(b"scl-rs/session-id/v1");
        input.extend_from_slice(&TAG);
        input.extend_from_slice(&2u64.to_le_bytes());
        input.extend_from_slice(&1u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        let expected: [u8; 32] = Sha256::digest(&input).into();
        assert_eq!(s.digest(), expected);
    }

    #[test]
    fn derived_root_tags_are_deterministic_and_separate_contexts() {
        assert_eq!(derive_root_tag(b"run-1"), derive_root_tag(b"run-1"));
        assert_ne!(derive_root_tag(b"run-1"), derive_root_tag(b"run-2"));
        assert_ne!(derive_root_tag(b""), derive_root_tag(b"\0"));
    }

    #[test]
    fn derived_root_tag_matches_its_documented_encoding() {
        let expected: [u8; 32] = Sha256::digest(b"scl-rs/root-tag/v1run-1").into();
        assert_eq!(derive_root_tag(b"run-1"), expected);
    }

    #[test]
    fn header_path_encoding_is_compact() {
        // What a frame header carries: a varint length, then one varint per index (each index
        // below 128 takes one byte). The root tag is not sent.
        assert_eq!(
            postcard::to_allocvec(SessionId::root(TAG).path()).unwrap(),
            vec![0]
        );
        assert_eq!(
            postcard::to_allocvec(session(&[1, 0]).path()).unwrap(),
            vec![2, 1, 0]
        );
    }

    #[test]
    fn header_round_trip_rebuilds_the_session() {
        for path in [&[][..], &[0], &[1, 0], &[7, 300, u32::MAX]] {
            let sent = session(path);
            let bytes = postcard::to_allocvec(sent.path()).unwrap();
            let decoded: Vec<u32> = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(SessionId::new(TAG, decoded), sent);
        }
    }

    #[test]
    fn display_shows_root_for_the_root_session() {
        assert_eq!(SessionId::root(TAG).to_string(), "root");
    }

    #[test]
    fn display_joins_the_path_with_dots() {
        assert_eq!(session(&[0]).to_string(), "0");
        assert_eq!(session(&[1, 0]).to_string(), "1.0");
        assert_eq!(session(&[3, 1, 4]).to_string(), "3.1.4");
        assert_eq!(session(&[2, u32::MAX]).to_string(), "2.4294967295");
    }

    #[test]
    fn display_matches_the_way_child_builds_the_path() {
        let s = SessionId::root(TAG).with_child(1).with_child(0);
        assert_eq!(s.to_string(), "1.0");
        assert_eq!(s.parent().unwrap().to_string(), "1");
        assert_eq!(s.parent().unwrap().parent().unwrap().to_string(), "root");
    }

    #[test]
    fn display_shows_only_the_path_not_the_root_tag() {
        let here = SessionId::root([1; 32]).with_child(1).with_child(0);
        let there = SessionId::root([2; 32]).with_child(1).with_child(0);
        assert_ne!(here, there);
        assert_eq!(here.to_string(), there.to_string());
    }

    #[test]
    fn display_respects_width_fill_and_alignment() {
        let root = SessionId::root(TAG);
        let nested = session(&[1, 0]);

        assert_eq!(format!("{root:>6}"), "  root");
        assert_eq!(format!("{root:<6}|"), "root  |");
        assert_eq!(format!("{nested:^7}"), "  1.0  ");
        assert_eq!(format!("{nested:*>5}"), "**1.0");
        // A width smaller than the text never truncates.
        assert_eq!(format!("{nested:>1}"), "1.0");
    }
}
