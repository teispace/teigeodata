//! A finite-state transducer over sorted byte keys: a minimal acyclic automaton whose path
//! outputs sum to each key's ordinal (its position in sorted order), after Daciuk, Mihov, Watson
//! and Watson (2000) and the `fst` crate's outputs, in a layout a reader can check
//! ([research note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/search-index.md)).
//!
//! Nodes are written children first and read backwards from their **address**, the offset of
//! their last byte, so every transition points to a lower address and every walk ends. The
//! root is the last node. All little-endian:
//!
//! ```text
//! leaf     header: bit 7 final, bits 0–6 zero
//! next     label u8, header (bit 7 final, bit 6 set, bits 0–5 zero): one transition to the
//!          node just below, at address − 2
//! one      delta [wt], label u8, header (bit 7 final, bit 5 set, bits 0–1 wt − 1, bits
//!          2–4 zero): one transition
//! general  outputs [(n − 1) × wo], deltas [n × wt], labels [n] strictly increasing,
//!          widths u8 (wt in bits 0–3, 1 to 4; wo in bits 4–7, 0 to 4), [n − 1 if n ≥ 31],
//!          header (bit 7 final, bits 5–6 clear, bits 0–4 n from 2 to 30, or 31)
//! ```
//!
//! A transition's target is `address − delta`, `delta ≥ 1`. Its output is the number of keys
//! before it in its node: 1 if the node is final, plus the keys under the transitions before
//! it. The first output is therefore 0 or 1 by the final bit and is not stored. The writer
//! uses the next form whenever it applies, else the one form for one transition, and the
//! smallest widths.

use core::ops::ControlFlow;

/// The longest key in bytes.
pub const MAX_KEY: usize = 255;

/// A check failure: the problem only (the section maps it to its error).
pub type Problem = &'static str;

/// A view of an automaton.
#[derive(Clone, Copy, Debug)]
pub struct Fst<'a> {
    bytes: &'a [u8],
    keys: u32,
}

/// One node, parsed: offsets into the automaton's bytes.
#[derive(Clone, Copy, Debug)]
struct Node<'a> {
    bytes: &'a [u8],
    addr: u32,
    /// The first byte of the node (its outputs).
    at: u32,
    n: u16,
    wt: u8,
    wo: u8,
    is_final: bool,
    /// The next form: one transition to `addr − 2`.
    next: bool,
}

const FINAL: u8 = 0x80;
const NEXT: u8 = 0x40;
const ONE: u8 = 0x20;
const COUNT: u8 = 0x1f;

fn le(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0, |v, &b| (v << 8) | u64::from(b))
}

impl<'a> Node<'a> {
    fn parse(bytes: &'a [u8], addr: usize) -> Option<Self> {
        let h = *bytes.get(addr)?;
        let a32 = u32::try_from(addr).ok()?;
        let is_final = h & FINAL != 0;
        let mut node = Self {
            bytes,
            addr: a32,
            at: a32,
            n: 0,
            wt: 0,
            wo: 0,
            is_final,
            next: false,
        };
        if h & NEXT != 0 {
            if h & 0x3f != 0 || addr < 2 {
                return None;
            }
            node.n = 1;
            node.at = a32 - 1;
            node.next = true;
            return Some(node);
        }
        if h & ONE != 0 {
            if h & 0x1c != 0 {
                return None;
            }
            node.n = 1;
            node.wt = (h & 0x03) + 1;
            node.at = a32.checked_sub(1 + u32::from(node.wt))?;
            return Some(node);
        }
        let low = h & COUNT;
        let (n, top) = match low {
            0 => return Some(node),
            1 => return None,
            COUNT => {
                let n = usize::from(*bytes.get(addr.checked_sub(1)?)?) + 1;
                if n < usize::from(COUNT) {
                    return None;
                }
                (n, addr - 1)
            }
            _ => (usize::from(low), addr),
        };
        let w = *bytes.get(top.checked_sub(1)?)?;
        let (wt, wo) = (w & 0x0f, w >> 4);
        if !(1..=4).contains(&wt) || wo > 4 {
            return None;
        }
        let at = (top - 1).checked_sub(n * usize::from(1 + wt) + (n - 1) * usize::from(wo))?;
        #[allow(clippy::cast_possible_truncation, reason = "n ≤ 256, at < addr")]
        {
            node.n = n as u16;
            node.at = at as u32;
        }
        (node.wt, node.wo) = (wt, wo);
        Some(node)
    }

    const fn n(&self) -> usize {
        self.n as usize
    }

    /// The node's size in bytes.
    const fn size(&self) -> usize {
        (self.addr - self.at) as usize + 1
    }

    /// Where the deltas start.
    fn deltas_at(&self) -> usize {
        self.at as usize + self.n().saturating_sub(1) * usize::from(self.wo)
    }

    fn labels(&self) -> &'a [u8] {
        let n = self.n();
        let at = self.deltas_at() + n * usize::from(self.wt);
        self.bytes.get(at..at + n).unwrap_or(&[])
    }

    fn label(&self, i: usize) -> Option<u8> {
        self.labels().get(i).copied()
    }

    fn target(&self, i: usize) -> Option<usize> {
        if self.next {
            return (i == 0).then(|| self.addr as usize - 2);
        }
        let wt = usize::from(self.wt);
        let at = self.deltas_at() + i * wt;
        let d = usize::try_from(le(self.bytes.get(at..at + wt)?)).ok()?;
        if d == 0 || i >= self.n() {
            return None;
        }
        (self.addr as usize).checked_sub(d)
    }

    fn output(&self, i: usize) -> Option<u32> {
        if i >= self.n() {
            return None;
        }
        if i == 0 {
            return Some(u32::from(self.is_final));
        }
        if self.wo == 0 {
            return Some(0);
        }
        let wo = usize::from(self.wo);
        let at = self.at as usize + (i - 1) * wo;
        u32::try_from(le(self.bytes.get(at..at + wo)?)).ok()
    }

    /// Transition `i`: its label, target and output, in one read of the node's offsets.
    #[inline]
    fn transition(&self, i: usize) -> Option<(u8, usize, u32)> {
        let n = self.n();
        if i >= n {
            return None;
        }
        if self.next {
            let label = *self.bytes.get(self.addr as usize - 1)?;
            return Some((label, self.addr as usize - 2, u32::from(self.is_final)));
        }
        let (wt, wo) = (usize::from(self.wt), usize::from(self.wo));
        let deltas = self.at as usize + (n - 1) * wo;
        let labels = deltas + n * wt;
        let label = *self.bytes.get(labels + i)?;
        let d = usize::try_from(le(self
            .bytes
            .get(deltas + i * wt..deltas + (i + 1) * wt)?))
        .ok()?;
        if d == 0 {
            return None;
        }
        let target = (self.addr as usize).checked_sub(d)?;
        let out = if i == 0 {
            u32::from(self.is_final)
        } else if wo == 0 {
            0
        } else {
            let at = self.at as usize + (i - 1) * wo;
            u32::try_from(le(self.bytes.get(at..at + wo)?)).ok()?
        };
        Some((label, target, out))
    }

    /// The transition labelled `b`, or the index of the first label above it.
    fn find(&self, b: u8) -> Result<usize, usize> {
        let labels = self.labels();
        if labels.len() <= 16 {
            for (i, &l) in labels.iter().enumerate() {
                if l >= b {
                    return if l == b { Ok(i) } else { Err(i) };
                }
            }
            Err(labels.len())
        } else {
            labels.binary_search(&b)
        }
    }
}

/// A key matcher for [`Fst::walk`]: an automaton over code points.
pub trait Matcher {
    /// Its state, kept per depth of the walk.
    type State: Copy;
    /// The state before any character.
    fn start(&self) -> Self::State;
    /// The state after `c`, or `None` when no continuation can match.
    fn step(&self, s: &Self::State, c: char) -> Option<Self::State>;
    /// The edits of a match, if the key read so far matches.
    fn accept(&self, s: &Self::State) -> Option<u8>;
    /// The most characters a matching key can have after the start.
    fn max_chars(&self) -> usize {
        MAX_KEY
    }
}

impl Matcher for super::fuzzy::Fuzzy {
    type State = super::fuzzy::State;
    fn start(&self) -> Self::State {
        Self::start(self)
    }
    fn step(&self, s: &Self::State, c: char) -> Option<Self::State> {
        let n = Self::step(self, s, c);
        self.alive(&n).then_some(n)
    }
    fn accept(&self, s: &Self::State) -> Option<u8> {
        self.distance(s)
    }
    fn max_chars(&self) -> usize {
        (self.len() + u32::from(self.edits())) as usize
    }
}

/// How a walk ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walked {
    /// Every match was visited.
    Done,
    /// The visitor stopped it.
    Stopped,
    /// The step budget ran out.
    Budget,
}

#[derive(Clone, Copy)]
struct Frame<'a, S> {
    node: Node<'a>,
    next: u16,
    ordinal: u32,
    state: S,
    /// Code point bits so far, and continuation bytes still due.
    partial: u32,
    due: u8,
    chars: u8,
}

/// The frames of a short walk; deeper walks use [`MAX_KEY`] + 1.
const SHORT: usize = 64;

impl<'a> Fst<'a> {
    /// A view of `bytes` holding `keys` keys, if its root node parses. [`Fst::check`] verifies
    /// the rest; queries on an unchecked automaton stay bounded and never panic.
    #[must_use]
    pub fn new(bytes: &'a [u8], keys: u32) -> Option<Self> {
        let f = Self { bytes, keys };
        Node::parse(bytes, bytes.len().checked_sub(1)?)?;
        Some(f)
    }

    /// The number of keys.
    #[must_use]
    pub const fn keys(&self) -> u32 {
        self.keys
    }

    /// The automaton's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    fn root(&self) -> usize {
        self.bytes.len().saturating_sub(1)
    }

    fn node(&self, addr: usize) -> Option<Node<'a>> {
        Node::parse(self.bytes, addr)
    }

    /// The ordinal of `key`, if present.
    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<u32> {
        let mut addr = self.root();
        let mut ordinal = 0_u32;
        for &b in key {
            let node = self.node(addr)?;
            let i = node.find(b).ok()?;
            ordinal = ordinal.checked_add(node.output(i)?)?;
            addr = node.target(i)?;
        }
        (self.node(addr)?.is_final && ordinal < self.keys).then_some(ordinal)
    }

    /// The ordinals `[lo, hi)` of the keys starting with `prefix`.
    #[must_use]
    pub fn range(&self, prefix: &[u8]) -> (u32, u32) {
        let mut addr = self.root();
        let (mut lo, mut hi) = (0_u32, self.keys);
        for &b in prefix {
            let Some(node) = self.node(addr) else {
                return (lo, lo);
            };
            match node.find(b) {
                Ok(i) => {
                    if let Some(after) = node.output(i + 1).filter(|_| i + 1 < node.n()) {
                        hi = hi.min(lo.saturating_add(after));
                    }
                    lo = lo.saturating_add(node.output(i).unwrap_or(0));
                    match node.target(i) {
                        Some(t) => addr = t,
                        None => return (lo, lo),
                    }
                }
                Err(i) => {
                    let at = if i < node.n() {
                        lo.saturating_add(node.output(i).unwrap_or(0))
                    } else {
                        hi
                    };
                    return (at, at);
                }
            }
        }
        (lo.min(hi), hi)
    }

    /// Visits, in key order, every key that starts with `prefix` and whose rest `matcher`
    /// accepts, with its ordinal and edits. `anchor` requires the rest to start with that
    /// character. At most `budget` transitions are followed.
    pub fn walk<M: Matcher>(
        &self,
        prefix: &[u8],
        matcher: &M,
        anchor: Option<char>,
        budget: usize,
        mut visit: impl FnMut(u32, u8) -> ControlFlow<()>,
    ) -> Walked {
        let mut buf = [0_u8; 4];
        let anchor_bytes = anchor.map_or(&[][..], |a| a.encode_utf8(&mut buf).as_bytes());
        let Some((addr, ordinal)) = self.follow(prefix.iter().chain(anchor_bytes)) else {
            return Walked::Done;
        };
        let mut state = matcher.start();
        if let Some(a) = anchor {
            match matcher.step(&state, a) {
                Some(s) => state = s,
                None => return Walked::Done,
            }
        }
        let Some(node) = self.node(addr) else {
            return Walked::Done;
        };
        if node.is_final
            && let Some(e) = matcher.accept(&state)
            && visit(ordinal, e).is_break()
        {
            return Walked::Stopped;
        }
        let start = Frame {
            node,
            next: 0,
            ordinal,
            state,
            partial: 0,
            due: 0,
            chars: 0,
        };
        let room = MAX_KEY.saturating_sub(prefix.len() + anchor_bytes.len());
        // A matching rest has at most this many bytes.
        let deepest = matcher.max_chars().saturating_mul(4).min(room);
        if deepest < SHORT {
            let mut stack = [start; SHORT];
            self.walk_in(&mut stack, deepest, matcher, budget, &mut visit)
        } else {
            let mut stack = [start; MAX_KEY + 1];
            self.walk_in(&mut stack, deepest, matcher, budget, &mut visit)
        }
    }

    /// The node and ordinal after `path`.
    fn follow<'p>(&self, path: impl Iterator<Item = &'p u8>) -> Option<(usize, u32)> {
        let mut addr = self.root();
        let mut ordinal = 0_u32;
        for &b in path {
            let node = self.node(addr)?;
            let i = node.find(b).ok()?;
            ordinal = ordinal.saturating_add(node.output(i)?);
            addr = node.target(i)?;
        }
        Some((addr, ordinal))
    }

    fn walk_in<M: Matcher>(
        &self,
        stack: &mut [Frame<'a, M::State>],
        deepest: usize,
        matcher: &M,
        budget: usize,
        visit: &mut impl FnMut(u32, u8) -> ControlFlow<()>,
    ) -> Walked {
        let mut depth = 0_usize;
        let mut steps = 0_usize;
        let limit = deepest.min(stack.len() - 1);
        loop {
            let Some(top) = stack.get_mut(depth) else {
                return Walked::Done;
            };
            let at = usize::from(top.next);
            if at >= top.node.n() || depth >= limit {
                if depth == 0 {
                    return Walked::Done;
                }
                depth -= 1;
                continue;
            }
            top.next += 1;
            steps += 1;
            if steps > budget {
                return Walked::Budget;
            }
            let Some((b, t, out)) = top.node.transition(at) else {
                continue;
            };
            let mut child = Frame {
                next: 0,
                ordinal: top.ordinal.saturating_add(out),
                ..*top
            };
            // Assemble the code point; an invalid sequence reads as U+FFFD.
            let done = if child.due == 0 {
                match b {
                    0x00..=0x7f => Some(u32::from(b)),
                    0xc0..=0xdf => {
                        (child.partial, child.due) = (u32::from(b & 0x1f), 1);
                        None
                    }
                    0xe0..=0xef => {
                        (child.partial, child.due) = (u32::from(b & 0x0f), 2);
                        None
                    }
                    0xf0..=0xf7 => {
                        (child.partial, child.due) = (u32::from(b & 0x07), 3);
                        None
                    }
                    _ => Some(0xfffd),
                }
            } else if b & 0xc0 == 0x80 {
                child.partial = (child.partial << 6) | u32::from(b & 0x3f);
                child.due -= 1;
                (child.due == 0).then_some(child.partial)
            } else {
                child.due = 0;
                Some(0xfffd)
            };
            if let Some(code) = done {
                let c = char::from_u32(code).unwrap_or('\u{fffd}');
                child.chars = child.chars.saturating_add(1);
                match matcher.step(&child.state, c) {
                    Some(s) => child.state = s,
                    None => continue,
                }
            }
            // Parsed only once the matcher keeps the transition: most die first.
            let Some(node) = self.node(t) else {
                continue;
            };
            child.node = node;
            if child.due == 0
                && node.is_final
                && let Some(e) = matcher.accept(&child.state)
                && visit(child.ordinal, e).is_break()
            {
                return Walked::Stopped;
            }
            depth += 1;
            if let Some(slot) = stack.get_mut(depth) {
                *slot = child;
            }
        }
    }

    /// Visits every key in order with its bytes; stops at the first problem. O(trie nodes);
    /// refuses more keys than [`Fst::keys`], so it is bounded on any input. A well-formed node
    /// that no transition reaches is not detected: it changes no answer, and the pack
    /// signature guards the bytes.
    ///
    /// # Errors
    /// The problem, when a node is malformed or not canonical, a transition does not point
    /// lower, labels do not increase, an output is not the running ordinal, a key is longer
    /// than [`MAX_KEY`], or the key count differs.
    pub fn check(&self, mut visit: impl FnMut(u32, &[u8])) -> Result<(), Problem> {
        if self.bytes.is_empty() {
            return Err("the automaton is empty");
        }
        // The nodes tile the bytes, from the root down.
        let mut at = self.root();
        loop {
            let node = self.node(at).ok_or("a node is malformed")?;
            Self::canonical(&node)?;
            match at.checked_sub(node.size()) {
                Some(below) => at = below,
                None if node.size() == at + 1 => break,
                None => return Err("a node runs past the start"),
            }
        }
        let mut key = [0_u8; MAX_KEY];
        // (node address, next transition, ordinal at the node, keys emitted on entry). Every
        // transition must lead to a key, so the walk follows at most keys × MAX_KEY of them.
        let mut stack = [(0_usize, 0_usize, 0_u32, 0_u32); MAX_KEY + 1];
        stack[0] = (self.root(), 0, 0, 0);
        let mut depth = 0_usize;
        let mut emitted = 0_u32;
        let mut enter = |addr: usize, ordinal: u32, key: &[u8], emitted: &mut u32| {
            let node = self.node(addr).ok_or("a node is malformed")?;
            if node.is_final {
                if ordinal != *emitted || *emitted >= self.keys {
                    return Err("a key's ordinal is wrong");
                }
                visit(ordinal, key);
                *emitted += 1;
            }
            Ok(node)
        };
        enter(self.root(), 0, &[], &mut emitted)?;
        loop {
            let Some(&(addr, next, ordinal, entered)) = stack.get(depth) else {
                return Err("a key is too long");
            };
            let node = self.node(addr).ok_or("a node is malformed")?;
            if next >= node.n() {
                if depth == 0 {
                    break;
                }
                if emitted == entered {
                    return Err("a transition leads to no key");
                }
                depth -= 1;
                continue;
            }
            if let Some(frame) = stack.get_mut(depth) {
                frame.1 += 1;
            }
            if depth >= MAX_KEY {
                return Err("a key is too long");
            }
            let label = node.label(next).ok_or("a label is missing")?;
            if next > 0 && node.label(next - 1) >= Some(label) {
                return Err("the labels do not increase");
            }
            let target = node
                .target(next)
                .ok_or("a transition does not point lower")?;
            let out = ordinal
                .checked_add(node.output(next).ok_or("an output is missing")?)
                .ok_or("an output overflows")?;
            // A wrong output shifts every key under the transition, and the transition leads to
            // at least one key, whose ordinal `enter` checks.
            let (Some(slot), Some(frame)) = (key.get_mut(depth), stack.get_mut(depth + 1)) else {
                return Err("a key is too long");
            };
            *slot = label;
            *frame = (target, 0, out, emitted);
            depth += 1;
            enter(target, out, key.get(..depth).unwrap_or(&[]), &mut emitted)?;
        }
        if emitted != self.keys {
            return Err("the key count differs");
        }
        Ok(())
    }

    /// The writer's encoding rules for one node.
    fn canonical(node: &Node<'_>) -> Result<(), Problem> {
        if node.next || node.n == 0 {
            return Ok(());
        }
        let mut max_delta = 0_u64;
        let mut max_out = 0_u64;
        for i in 0..node.n() {
            let t = node.target(i).ok_or("a transition does not point lower")?;
            max_delta = max_delta.max((node.addr as usize - t) as u64);
            if i > 0 {
                max_out = max_out.max(u64::from(node.output(i).ok_or("an output is missing")?));
            }
        }
        if width(max_delta).max(1) != usize::from(node.wt)
            || (node.n > 1 && width(max_out) != usize::from(node.wo))
        {
            return Err("a node's widths are not the smallest");
        }
        if node.n == 1 && max_delta == node.size() as u64 {
            return Err("a node should use the next form");
        }
        Ok(())
    }
}

/// Bytes needed for `v` (0 for 0).
fn width(v: u64) -> usize {
    (64 - v.leading_zeros() as usize).div_ceil(8)
}

#[cfg(feature = "write")]
pub use build::Builder;

#[cfg(feature = "write")]
#[allow(
    clippy::indexing_slicing,
    reason = "the writer's indices are bounded by construction: open nodes per key byte, \
              registry slots modulo its length, outputs per transition"
)]
mod build {
    use alloc::vec::Vec;

    use super::{COUNT, FINAL, MAX_KEY, NEXT, Node, ONE, width};
    use crate::Error;

    /// One node of the last key, not yet written.
    #[derive(Default)]
    struct Open {
        is_final: bool,
        /// Written transitions: (label, target address, keys under it).
        done: Vec<(u8, usize, u32)>,
        /// The label towards the next open node.
        pending: Option<u8>,
    }

    /// Builds an automaton from keys in strictly increasing order, deterministically, in
    /// memory bounded by the output and a fixed registry of written nodes.
    pub struct Builder {
        out: Vec<u8>,
        open: Vec<Open>,
        last: Vec<u8>,
        keys: u32,
        /// Hash → (address, keys under it), direct-mapped: a node equal to a recent one is
        /// shared. A miss only costs size, never correctness.
        registry: Vec<(u64, usize, u32)>,
        last_addr: Option<usize>,
    }

    fn fail(problem: &'static str) -> Error {
        Error::Write(problem)
    }

    impl core::fmt::Debug for Builder {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("Builder")
                .field("keys", &self.keys)
                .field("bytes", &self.out.len())
                .finish_non_exhaustive()
        }
    }

    impl Default for Builder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Builder {
        /// An empty builder with a registry of 2¹⁸ nodes.
        #[must_use]
        pub fn new() -> Self {
            Self {
                out: Vec::new(),
                open: alloc::vec![Open::default()],
                last: Vec::new(),
                keys: 0,
                registry: alloc::vec![(0, usize::MAX, 0); 1 << 18],
                last_addr: None,
            }
        }

        /// Adds `key`, which must sort after the previous one.
        ///
        /// # Errors
        /// [`Error::Write`] if the key is not after the previous one, is longer than
        /// [`MAX_KEY`], or there are more than `u32::MAX` keys.
        pub fn insert(&mut self, key: &[u8]) -> Result<(), Error> {
            if key.len() > MAX_KEY {
                return Err(fail("a search key is too long"));
            }
            if self.keys > 0 && key <= self.last.as_slice() {
                return Err(fail("the search keys are not strictly increasing"));
            }
            if self.keys == 0 && key.is_empty() && self.open[0].is_final {
                return Err(fail("the search keys are not strictly increasing"));
            }
            let shared = self
                .last
                .iter()
                .zip(key)
                .take_while(|(a, b)| a == b)
                .count();
            self.close(shared)?;
            for (d, &b) in key.iter().enumerate().skip(shared) {
                self.open[d].pending = Some(b);
                debug_assert_eq!(self.open.len(), d + 1);
                self.open.push(Open::default());
            }
            if let Some(n) = self.open.get_mut(key.len()) {
                n.is_final = true;
            }
            self.last.clear();
            self.last.extend_from_slice(key);
            self.keys = self
                .keys
                .checked_add(1)
                .ok_or(fail("too many search keys"))?;
            Ok(())
        }

        /// Writes the open nodes deeper than `depth`.
        fn close(&mut self, depth: usize) -> Result<(), Error> {
            while self.open.len() > depth + 1 {
                let node = self.open.pop().ok_or(fail("no open node"))?;
                let (addr, count) = self.compile(&node, true)?;
                let parent = self.open.last_mut().ok_or(fail("no open node"))?;
                let label = parent.pending.take().ok_or(fail("no pending label"))?;
                parent.done.push((label, addr, count));
            }
            Ok(())
        }

        /// The automaton's bytes and its key count.
        ///
        /// # Errors
        /// [`Error::Write`] if a node cannot be encoded.
        pub fn finish(mut self) -> Result<(Vec<u8>, u32), Error> {
            self.close(0)?;
            let root = self.open.pop().ok_or(fail("no open node"))?;
            // The root is always written last, never shared, so it is the last byte.
            self.compile(&root, false)?;
            Ok((self.out, self.keys))
        }

        /// Writes `node` (or finds an equal written node) and returns its address and keys.
        fn compile(&mut self, node: &Open, share: bool) -> Result<(usize, u32), Error> {
            let mut count = u32::from(node.is_final);
            let mut outs = Vec::with_capacity(node.done.len());
            for &(_, _, c) in &node.done {
                outs.push(count);
                count = count.checked_add(c).ok_or(fail("too many search keys"))?;
            }
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            let mut mix = |v: u64| {
                for b in v.to_le_bytes() {
                    h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
                }
            };
            mix(u64::from(node.is_final));
            for (&(l, t, _), &o) in node.done.iter().zip(&outs) {
                mix(u64::from(l));
                mix(t as u64);
                mix(u64::from(o));
            }
            let slot = usize::try_from(h % self.registry.len() as u64).unwrap_or(0);
            if share {
                let (sh, sa, sc) = self.registry[slot];
                if sh == h && sa != usize::MAX && self.same(sa, node, &outs) {
                    return Ok((sa, sc));
                }
            }
            let addr = self.write(node, &outs)?;
            if share {
                self.registry[slot] = (h, addr, count);
            }
            self.last_addr = Some(addr);
            Ok((addr, count))
        }

        fn same(&self, addr: usize, node: &Open, outs: &[u32]) -> bool {
            let Some(n) = Node::parse(&self.out, addr) else {
                return false;
            };
            n.is_final == node.is_final
                && n.n() == node.done.len()
                && node
                    .done
                    .iter()
                    .zip(outs)
                    .enumerate()
                    .all(|(i, (&(l, t, _), &o))| {
                        n.label(i) == Some(l) && n.target(i) == Some(t) && n.output(i) == Some(o)
                    })
        }

        fn write(&mut self, node: &Open, outs: &[u32]) -> Result<usize, Error> {
            let head = if node.is_final { FINAL } else { 0 };
            let n = node.done.len();
            let start = self.out.len();
            if n == 0 {
                self.out.push(head);
                return Ok(start);
            }
            if n == 1 {
                let (label, target, _) = node.done[0];
                if self.last_addr == Some(target) {
                    self.out.push(label);
                    self.out.push(head | NEXT);
                    return Ok(start + 1);
                }
                let mut wt = 1;
                while width((start + wt + 1 - target) as u64) > wt {
                    wt += 1;
                }
                if wt > 4 {
                    return Err(fail("the search automaton is too large"));
                }
                let addr = start + wt + 1;
                self.out
                    .extend_from_slice(&((addr - target) as u64).to_le_bytes()[..wt]);
                self.out.push(label);
                #[allow(clippy::cast_possible_truncation, reason = "wt ≤ 4")]
                self.out.push(head | ONE | (wt as u8 - 1));
                return Ok(addr);
            }
            let wo = width(outs.iter().skip(1).copied().max().map_or(0, u64::from));
            let extra = usize::from(n >= usize::from(COUNT));
            let min_target = node.done.iter().map(|d| d.1).min().unwrap_or(0);
            let mut wt = 1;
            let addr = loop {
                let size = 1 + extra + 1 + n + n * wt + (n - 1) * wo;
                let addr = start + size - 1;
                if width((addr - min_target) as u64) <= wt {
                    break addr;
                }
                wt += 1;
                if wt > 4 {
                    return Err(fail("the search automaton is too large"));
                }
            };
            for &o in outs.iter().skip(1) {
                self.out.extend_from_slice(&o.to_le_bytes()[..wo]);
            }
            for &(_, t, _) in &node.done {
                self.out
                    .extend_from_slice(&((addr - t) as u64).to_le_bytes()[..wt]);
            }
            self.out.extend(node.done.iter().map(|d| d.0));
            #[allow(clippy::cast_possible_truncation, reason = "widths are at most 4")]
            self.out.push((wt as u8) | ((wo as u8) << 4));
            if extra == 1 {
                #[allow(clippy::cast_possible_truncation, reason = "n ≤ 256")]
                self.out.push((n - 1) as u8);
                self.out.push(head | COUNT);
            } else {
                #[allow(clippy::cast_possible_truncation, reason = "n < 31")]
                self.out.push(head | n as u8);
            }
            debug_assert_eq!(self.out.len() - 1, addr);
            Ok(addr)
        }
    }
}

#[cfg(all(test, feature = "write"))]
#[allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::needless_range_loop,
    clippy::manual_strip,
    clippy::range_plus_one,
    reason = "tests: the oracles mirror the textbook dynamic program; inputs are cut from a 64-bit generator"
)]
mod tests {
    use core::ops::ControlFlow;
    use std::collections::BTreeSet;
    use std::string::String;
    use std::vec::Vec;

    use super::{Builder, Fst, MAX_KEY, Walked};
    use crate::search::fuzzy::Fuzzy;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Words over a small alphabet with shared prefixes and suffixes, some Devanagari.
    fn words(r: &mut Rng, n: usize) -> Vec<String> {
        const PARTS: [&str; 10] = [
            "ka",
            "kha",
            "pur",
            "gaun",
            "a",
            "nagar",
            "क",
            "पुर",
            "ि",
            "z",
        ];
        let mut set = BTreeSet::new();
        for _ in 0..n {
            let k = 1 + r.below(4);
            let w: String = (0..k).map(|_| PARTS[r.below(PARTS.len())]).collect();
            set.insert(w);
        }
        set.into_iter().collect()
    }

    fn build(keys: &[String]) -> Vec<u8> {
        let mut b = Builder::new();
        for k in keys {
            b.insert(k.as_bytes()).unwrap();
        }
        let (bytes, n) = b.finish().unwrap();
        assert_eq!(n as usize, keys.len());
        bytes
    }

    fn osa(a: &str, b: &str) -> usize {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let mut d = std::vec![std::vec![0_usize; b.len() + 1]; a.len() + 1];
        for i in 0..=a.len() {
            d[i][0] = i;
        }
        for j in 0..=b.len() {
            d[0][j] = j;
        }
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                let mut v = (d[i - 1][j] + 1)
                    .min(d[i][j - 1] + 1)
                    .min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    v = v.min(d[i - 2][j - 2] + 1);
                }
                d[i][j] = v;
            }
        }
        d[a.len()][b.len()]
    }

    #[test]
    fn lookups_agree_with_the_sorted_keys() {
        let mut r = Rng(11);
        // Under Miri (about a million times slower), shapes up to 40 keys and fewer probes.
        let sizes: &[usize] = if cfg!(miri) {
            &[0, 1, 2, 5, 40]
        } else {
            &[0, 1, 2, 5, 40, 600]
        };
        for &n in sizes {
            let keys = words(&mut r, n);
            let bytes = build(&keys);
            let f = Fst::new(&bytes, keys.len() as u32).unwrap();
            let mut seen = Vec::new();
            f.check(|o, k| seen.push((o, k.to_vec()))).unwrap();
            let want: Vec<(u32, Vec<u8>)> = keys
                .iter()
                .enumerate()
                .map(|(i, k)| (i as u32, k.as_bytes().to_vec()))
                .collect();
            assert_eq!(seen, want);
            for (i, k) in keys.iter().enumerate() {
                assert_eq!(f.get(k.as_bytes()), Some(i as u32), "{k}");
            }
            // Absent keys and every prefix range.
            for probe in words(&mut r, if cfg!(miri) { 8 } else { 200 }) {
                let at = keys.binary_search(&probe);
                assert_eq!(f.get(probe.as_bytes()), at.ok().map(|i| i as u32));
                for cut in 0..=probe.len() {
                    let Some(p) = probe.get(..cut) else { continue };
                    let lo = keys.partition_point(|k| k.as_str() < p);
                    let hi = lo + keys[lo..].iter().take_while(|k| k.starts_with(p)).count();
                    assert_eq!(f.range(p.as_bytes()), (lo as u32, hi as u32), "{p:?}");
                }
            }
        }
    }

    #[test]
    fn walks_agree_with_brute_force() {
        let mut r = Rng(5);
        let keys = words(&mut r, if cfg!(miri) { 200 } else { 3000 });
        // Tagged keys: a tag byte before every word, two tags.
        let mut tagged: Vec<String> = keys
            .iter()
            .flat_map(|k| [std::format!("\u{1}{k}"), std::format!("\u{2}{k}")])
            .collect();
        tagged.sort();
        let bytes = build(&tagged);
        let f = Fst::new(&bytes, tagged.len() as u32).unwrap();
        f.check(|_, _| {}).unwrap();
        for q in words(&mut r, if cfg!(miri) { 3 } else { 150 }) {
            for edits in 0..=2_u8 {
                let fz = Fuzzy::new(&q, edits).unwrap();
                for anchor in [None, q.chars().next()] {
                    let mut got = Vec::new();
                    let w = f.walk(&[2], &fz, anchor, usize::MAX, |o, e| {
                        got.push((o, e));
                        ControlFlow::Continue(())
                    });
                    assert_eq!(w, Walked::Done);
                    let want: Vec<(u32, u8)> = tagged
                        .iter()
                        .enumerate()
                        .filter_map(|(i, t)| {
                            let k = t.strip_prefix('\u{2}')?;
                            if anchor.is_some_and(|a| !k.starts_with(a)) {
                                return None;
                            }
                            let d = osa(&q, k);
                            (d <= usize::from(edits)).then_some((i as u32, d as u8))
                        })
                        .collect();
                    assert_eq!(got, want, "{q:?} edits {edits} anchor {anchor:?}");
                }
            }
        }
        // A budget and a stop end the walk early.
        let fz = Fuzzy::new("kapur", 2).unwrap();
        assert_eq!(
            f.walk(&[2], &fz, None, 3, |_, _| ControlFlow::Continue(())),
            Walked::Budget
        );
        assert_eq!(
            f.walk(&[2], &fz, None, usize::MAX, |_, _| ControlFlow::Break(())),
            Walked::Stopped
        );
    }

    #[test]
    fn the_writer_refuses_and_shares() {
        let mut b = Builder::new();
        b.insert(b"b").unwrap();
        assert!(b.insert(b"a").is_err());
        assert!(b.insert(b"b").is_err());
        assert!(b.insert(&[b'c'; MAX_KEY + 1]).is_err());
        // Shared suffixes: many keys ending alike take far fewer bytes than their text.
        let n = if cfg!(miri) { 300 } else { 2000 };
        let keys: Vec<String> = (0..n).map(|i| std::format!("{i:05}nagar")).collect();
        let bytes = build(&keys);
        assert!(
            bytes.len() * 4 < keys.iter().map(String::len).sum::<usize>(),
            "{}",
            bytes.len()
        );
        // Deterministic.
        assert_eq!(build(&keys), bytes);
    }

    #[test]
    fn non_canonical_nodes_are_refused() {
        // "ab": a final leaf at 0, the node for `b` at 2 (next form), the root at 4.
        let good = [0x80, b'b', 0x40, b'a', 0x40];
        let f = Fst::new(&good, 1).unwrap();
        f.check(|_, _| {}).unwrap();
        assert_eq!(f.get(b"ab"), Some(0));
        assert_eq!(build(&[String::from("ab")]), good);
        let refuse = |bytes: &[u8], keys: u32, why: &str| {
            let f = Fst::new(bytes, keys).unwrap();
            assert_eq!(f.check(|_, _| {}), Err(why), "{bytes:?}");
        };
        // The root in the one form where the next form applies: delta 3, the node size.
        refuse(
            &[0x80, b'b', 0x40, 3, b'a', 0x20],
            1,
            "a node should use the next form",
        );
        // A delta width wider than needed.
        refuse(
            &[0x80, b'b', 0x40, 4, 0, b'a', 0x21],
            1,
            "a node's widths are not the smallest",
        );
        // One transition in the general form does not parse.
        assert!(Fst::new(&[0x80, b'b', 0x40, 4, b'a', 0x01, 0x01], 1).is_none());
        // A node with no key under it.
        refuse(&[0x00, b'a', 0x40], 0, "a transition leads to no key");
        // Labels out of order.
        refuse(
            &[0x80, 1, 7, 7, b'b', b'a', 0x11, 0x02],
            2,
            "the labels do not increase",
        );
    }

    #[test]
    fn hostile_bytes_never_panic_or_run_away() {
        let mut r = Rng(3);
        let keys = words(&mut r, 400);
        let bytes = build(&keys);
        let n = keys.len() as u32;
        let cases = if cfg!(miri) { 20 } else { 20_000 };
        let fz = Fuzzy::new("kapur", 2).unwrap();
        let mut refused = 0;
        for _ in 0..cases {
            let mut b = bytes.clone();
            for _ in 0..1 + r.below(3) {
                let i = r.below(b.len());
                b[i] = r.next() as u8;
            }
            let Some(f) = Fst::new(&b, n) else { continue };
            let ok = f.check(|_, _| {}).is_ok();
            refused += usize::from(!ok);
            let _ = f.get(b"kapur");
            let _ = f.range(b"ka");
            let _ = f.walk(&[], &fz, None, 100_000, |_, _| ControlFlow::Continue(()));
        }
        assert!(refused > cases / 2, "{refused}");
    }
}
