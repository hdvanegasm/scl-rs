//! The tree of protocol scopes of one party's trace, rebuilt from its events.
//!
//! Both the bandwidth tree and the trace's `Display` are read off this tree, so they agree on how
//! scopes nest when the branches of a combinator interleave in the trace.

use std::{
    collections::HashMap,
    fmt::{self, Formatter},
};

use crate::{
    net::{session::SessionId, simulation::event::Event},
    protocol::ProtocolId,
};

/// Index of the root node in the arena of nodes.
///
/// The root is synthetic: no `ProtocolBegin` opens it. It spans the whole trace and holds what
/// happens outside every protocol scope.
pub(crate) const ROOT: usize = 0;

/// One thing recorded inside a scope: either one of its own events or one of its child scopes.
///
/// Both variants hold an index, not a reference. The nodes live in one arena (a `Vec`), and a node
/// points to its children by their index in it.
pub(crate) enum Item {
    /// The event at this index of the trace.
    Event(usize),
    /// The child scope whose node is at this index of the set of nodes in the tree.
    Child(usize),
}

/// One protocol scope: it is what lies between `ProtocolBegin` and `ProtocolEnd`.
pub(crate) struct Node {
    /// The protocol's name.
    ///
    /// This field would be `None` for the root.
    pub(crate) id: Option<ProtocolId>,
    /// The session the scope was opened in: the session of its `ProtocolBegin`.
    ///
    /// This field is `None` for the root.
    pub(crate) session: Option<SessionId>,
    /// Payload bytes sent in this scope itself, excluding its children and self-sends.
    pub(crate) self_bytes: usize,
    /// The scope's own events and its child scopes, in the order of the trace.
    ///
    /// They are pushed during a single pass over the trace, so they are already in trace order and
    /// need no sorting.
    pub(crate) items: Vec<Item>,
    /// Whether a `ProtocolEnd` closed the scope.
    ///
    /// This is False for the root, and for a scope whose protocol was droped before it finished.
    pub(crate) ended: bool,
}

/// Something in a trace that does not fit a tree of scopes.
pub(crate) struct Anomaly {
    /// Index of the event in the trace.
    pub(crate) event_index: usize,
    /// Reason of the anomaly.
    pub(crate) reason: String,
}

/// The trace tree.
pub(crate) struct TraceTree {
    /// The nodes in the trace tree.
    ///
    /// A node's children always have a higher index than the node.
    pub(crate) nodes: Vec<Node>,
    /// List of anomalies in this tree.
    pub(crate) anomalies: Vec<Anomaly>,
}

/// Tells whether `session` is below `ancestor` in the trace tree.
fn is_below(session: &SessionId, ancestor: &SessionId) -> bool {
    session.root_tag() == ancestor.root_tag()
        && session.path().len() > ancestor.path().len()
        && session.path().starts_with(ancestor.path())
}

impl TraceTree {
    /// Builds the trace tree from a pass over `events`.
    pub(crate) fn build(events: &[Event]) -> Self {
        // Create the list of nodes with the root in it.
        let mut nodes = vec![Node {
            id: None,
            session: None,
            self_bytes: 0,
            items: Vec::new(),
            ended: false,
        }];

        let mut anomalies = Vec::new();

        // Tracker for the scopes that are open in each session.
        let mut open_scopes = HashMap::new();
        for (index, event) in events.iter().enumerate() {
            match event {
                Event::ProtocolBegin {
                    session,
                    protocol_name,
                    ..
                } => {
                    let parent = Self::innermost(&open_scopes, Some(session));
                    let node = nodes.len();
                    nodes.push(Node {
                        id: Some(*protocol_name),
                        session: Some(session.clone()),
                        self_bytes: 0,
                        items: Vec::new(),
                        ended: false,
                    });
                    nodes[parent].items.push(Item::Child(node));
                    open_scopes.entry(session.clone()).or_default().push(node);
                }
                Event::ProtocolEnd {
                    session,
                    protocol_name,
                    ..
                } => {
                    let innermost = open_scopes
                        .get(session)
                        .and_then(|stack| stack.last().copied());
                    match innermost {
                        Some(node) if nodes[node].id == Some(*protocol_name) => {
                            // The scope has finished, hence we mark it as ended and remove it from
                            // open scopes.
                            nodes[node].ended = true;
                            open_scopes.get_mut(session).map(Vec::pop);
                            open_scopes.retain(|other, _| !is_below(other, session));
                        }
                        Some(node) => {
                            let open_name = nodes[node].id.map(String::from).unwrap_or_default();
                            anomalies.push(Anomaly {
                                event_index: index,
                                reason: format!("`{protocol_name}` ends in session {session}, where the innermost open scope is {open_name}"),
                            });
                            nodes[node].items.push(Item::Event(index));
                        }
                        None => {
                            anomalies.push(Anomaly {
                                event_index: index,
                                reason: format!(
                                    "`{protocol_name}` ends in session {session}, where no scope \
                                     is open"
                                ),
                            });
                            let node = Self::innermost(&open_scopes, Some(session));
                            nodes[node].items.push(Item::Event(index));
                        }
                    }
                }
                Event::SendData {
                    session,
                    link,
                    size,
                    ..
                } => {
                    let node = Self::innermost(&open_scopes, Some(session));
                    // Self-sends never touch the wire, so they do not count as bandwidth.
                    if link.sender() != link.recipient() {
                        nodes[node].self_bytes += size;
                    }
                    nodes[node].items.push(Item::Event(index));
                }
                Event::ReceiveData { session, .. } => {
                    let node = Self::innermost(&open_scopes, Some(session));
                    nodes[node].items.push(Item::Event(index));
                }
                _ => {
                    let node = Self::innermost(&open_scopes, None);
                    nodes[node].items.push(Item::Event(index));
                }
            }
        }

        Self { nodes, anomalies }
    }

    /// Writes the trace as nested blocks, one per scope.
    pub(crate) fn render(&self, events: &[Event], f: &mut Formatter<'_>) -> fmt::Result {
        self.render_items(ROOT, 0, events, f, &mut true)
    }

    /// Writes the items of the scope at `node`, at indentation `depth`: each event as one line,
    /// and each child scope as a block.
    ///
    /// A block opens with the protocol's name, then ` [session …]` if it is the first scope of a
    /// branch (its session is not a root session, and not the session of the scope around it). It
    /// closes with `}`, followed by ` (not finished)` if no `ProtocolEnd` closed the scope.
    /// `first_line` is true until the first line is written, so that no line break precedes it.
    fn render_items(
        &self,
        node: usize,
        depth: usize,
        events: &[Event],
        f: &mut Formatter<'_>,
        first_line: &mut bool,
    ) -> fmt::Result {
        for item in &self.nodes[node].items {
            start_line(f, depth, first_line)?;
            match item {
                Item::Event(index) => write!(f, "{}", events[*index])?,
                Item::Child(child) => {
                    let scope = &self.nodes[*child];
                    if let Some(id) = scope.id {
                        write!(f, "{id}")?;
                    }
                    // A scope names its session when it is the first scope of a branch: its
                    // session is not the one of the scope around it.
                    if let Some(session) = &scope.session {
                        if session.depth() > 0 && self.nodes[node].session.as_ref() != Some(session)
                        {
                            write!(f, " [session {session}]")?;
                        }
                    }
                    write!(f, " {{")?;
                    self.render_items(*child, depth + 1, events, f, first_line)?;
                    start_line(f, depth, first_line)?;
                    write!(f, "}}")?;
                    if !scope.ended {
                        write!(f, " (not finished)")?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The innermost scope open for `session`: the top of its own stack, or else of its nearest
    /// ancestor's, which is where the combinator that forked it is open. Events that carry no
    /// session belong to the innermost scope of a root session.
    fn innermost(
        open_scopes: &HashMap<SessionId, Vec<usize>>,
        session: Option<&SessionId>,
    ) -> usize {
        let Some(session) = session else {
            return open_scopes
                .iter()
                .filter(|(session, _)| session.depth() == 0)
                .filter_map(|(_, stack)| stack.last().copied())
                .max()
                .unwrap_or(ROOT);
        };
        let mut current = Some(session.clone());
        while let Some(session) = current {
            if let Some(node) = open_scopes.get(&session).and_then(|stack| stack.last()) {
                return *node;
            }
            current = session.parent();
        }
        ROOT
    }
}

/// Starts a line at `depth`: a line break, unless this is the first line, then the indentation.
fn start_line(f: &mut Formatter<'_>, depth: usize, first_line: &mut bool) -> fmt::Result {
    const INDENT: &str = "    ";
    if !std::mem::replace(first_line, false) {
        writeln!(f)?;
    }
    (0..depth).try_for_each(|_| f.write_str(INDENT))
}
