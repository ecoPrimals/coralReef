// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::*;
use coral_reef_stubs::fxhash::{FxHashMap, FxHashSet};
use std::cmp::max;
use std::hash::Hash;
use std::ops::Range;
use std::slice;

#[derive(Clone)]
pub(super) enum RegUse<T: Clone> {
    None,
    Write(T),
    Reads(Vec<T>),
}

impl<T: Clone> RegUse<T> {
    pub fn deps(&self) -> &[T] {
        match self {
            Self::None => &[],
            Self::Write(dep) => slice::from_ref(dep),
            Self::Reads(deps) => &deps[..],
        }
    }

    pub const fn clear(&mut self) -> Self {
        std::mem::replace(self, Self::None)
    }

    pub const fn clear_write(&mut self) -> Self {
        if matches!(self, Self::Write(_)) {
            std::mem::replace(self, Self::None)
        } else {
            Self::None
        }
    }

    pub fn add_read(&mut self, dep: T) -> Self {
        match self {
            Self::None => {
                *self = Self::Reads(vec![dep]);
                Self::None
            }
            Self::Write(_) => std::mem::replace(self, Self::Reads(vec![dep])),
            Self::Reads(reads) => {
                reads.push(dep);
                Self::None
            }
        }
    }

    pub const fn set_write(&mut self, dep: T) -> Self {
        std::mem::replace(self, Self::Write(dep))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RegReadWrite {
    Read,
    Write,
}

/// Maps each register read/write to a value
/// a register can have multiple reads AND multiple writes at the same
/// point in time if it comes from a merge.
/// For edits inside a CFG block, a RegUseMap will never contain multiple
/// writes.
///
/// We need to track multiple reads as we don't know which one can cause
/// the highest latency for the interfering instruction (in RaW).  For the
/// same reason we might need to track both reads and writes in the case of
/// a CFG block with multiple successors.
/// We cannot flush writes after a read operation since we can still
/// encounter other, slower reads that could interfere with the write.
#[derive(Clone, PartialEq, Eq, Default)]
pub(super) struct RegUseMap<K: Hash + Eq, V> {
    map: FxHashMap<(RegReadWrite, K), V>,
}

impl<K, V> RegUseMap<K, V>
where
    K: Copy + Default + Hash + Eq,
    V: Clone,
{
    pub fn add_read(&mut self, k: K, v: V) {
        self.map.insert((RegReadWrite::Read, k), v);
    }

    pub fn set_write(&mut self, k: K, v: V) {
        // Writes wait on all previous Reads and writes
        self.map.clear();
        self.map.insert((RegReadWrite::Write, k), v);
    }

    pub fn iter_reads(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map
            .iter()
            .filter(|(k, _v)| k.0 == RegReadWrite::Read)
            .map(|(k, v)| (&k.1, v))
    }

    pub fn iter_writes(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map
            .iter()
            .filter(|(k, _v)| k.0 == RegReadWrite::Write)
            .map(|(k, v)| (&k.1, v))
    }

    pub fn retain(&mut self, f: impl FnMut(&(RegReadWrite, K), &mut V) -> bool) {
        self.map.retain(f);
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Merge two instances using a custom merger for value conflicts
    pub fn merge_with(&mut self, other: &Self, mut merger: impl FnMut(&V, &V) -> V) {
        use std::collections::hash_map::Entry;
        for (k, v) in &other.map {
            match self.map.entry(*k) {
                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert(v.clone());
                }
                Entry::Occupied(mut occupied_entry) => {
                    let orig = occupied_entry.get_mut();
                    *orig = merger(orig, v);
                }
            }
        }
    }
}

struct DepNode {
    read_dep: Option<usize>,
    first_wait: Option<(usize, usize)>,
}

pub(super) struct DepGraph {
    deps: Vec<DepNode>,
    instr_deps: FxHashMap<(usize, usize), (usize, usize)>,
    instr_waits: FxHashMap<(usize, usize), Vec<usize>>,
    active: FxHashSet<usize>,
}

impl DepGraph {
    pub fn new() -> Self {
        Self {
            deps: Vec::new(),
            instr_deps: FxHashMap::default(),
            instr_waits: FxHashMap::default(),
            active: FxHashSet::default(),
        }
    }

    fn add_new_dep(&mut self, read_dep: Option<usize>) -> usize {
        let dep = self.deps.len();
        self.deps.push(DepNode {
            read_dep,
            first_wait: None,
        });
        dep
    }

    pub fn add_instr(&mut self, block_idx: usize, ip: usize) -> (usize, usize) {
        let rd = self.add_new_dep(None);
        let wr = self.add_new_dep(Some(rd));
        self.instr_deps.insert((block_idx, ip), (rd, wr));
        (rd, wr)
    }

    pub fn add_signal(&mut self, dep: usize) {
        self.active.insert(dep);
    }

    pub fn add_waits(&mut self, block_idx: usize, ip: usize, mut waits: Vec<usize>) {
        for dep in &waits {
            // A wait on a write automatically waits on the read.  By removing
            // it from the active set here we ensure that we don't record any
            // duplicate write/read waits in the retain below.
            if let Some(rd) = &self.deps[*dep].read_dep {
                self.active.remove(rd);
            }
        }

        waits.retain(|dep| {
            let node = &mut self.deps[*dep];
            if let Some(wait) = node.first_wait {
                // Someone has already waited on this dep
                debug_assert!(!self.active.contains(dep));
                debug_assert!((block_idx, ip) >= wait);
                false
            } else if !self.active.contains(dep) {
                // Even if it doesn't have a use, it may still be deactivated.
                // This can happen if we depend the the destination before any
                // of its sources.
                false
            } else {
                self.deps[*dep].first_wait = Some((block_idx, ip));
                self.active.remove(dep);
                true
            }
        });

        // Sort for stability.  The list of waits may come from a HashSet (see
        // add_barrier()) and so it's not guaranteed stable across Rust
        // versions.  This also ensures that everything always waits on oldest
        // dependencies first.
        waits.sort();

        let _old = self.instr_waits.insert((block_idx, ip), waits);
        debug_assert!(_old.is_none());
    }

    pub fn add_barrier(&mut self, block_idx: usize, ip: usize) {
        let waits = self.active.iter().copied().collect();
        self.add_waits(block_idx, ip, waits);
        debug_assert!(self.active.is_empty());
    }

    pub fn dep_is_waited_after(&self, dep: usize, block_idx: usize, ip: usize) -> bool {
        self.deps[dep]
            .first_wait
            .is_some_and(|wait| wait > (block_idx, ip))
    }

    pub fn get_instr_deps(&self, block_idx: usize, ip: usize) -> (usize, usize) {
        *self
            .instr_deps
            .get(&(block_idx, ip))
            .expect("instruction must have dependency info")
    }

    pub fn get_instr_waits(&self, block_idx: usize, ip: usize) -> &[usize] {
        if let Some(waits) = self.instr_waits.get(&(block_idx, ip)) {
            &waits[..]
        } else {
            &[]
        }
    }
}

pub(super) struct BarAlloc {
    num_bars: u8,
    bar_dep: [usize; 6],
}

impl BarAlloc {
    pub const fn new() -> Self {
        Self {
            num_bars: 6,
            bar_dep: [usize::MAX; 6],
        }
    }

    pub fn bar_is_free(&self, bar: u8) -> bool {
        debug_assert!(bar < self.num_bars);
        self.bar_dep[usize::from(bar)] == usize::MAX
    }

    pub fn set_bar_dep(&mut self, bar: u8, dep: usize) {
        debug_assert!(self.bar_is_free(bar));
        self.bar_dep[usize::from(bar)] = dep;
    }

    pub fn free_bar(&mut self, bar: u8) {
        debug_assert!(!self.bar_is_free(bar));
        self.bar_dep[usize::from(bar)] = usize::MAX;
    }

    pub fn try_find_free_bar(&self) -> Option<u8> {
        (0..self.num_bars).find(|&bar| self.bar_is_free(bar))
    }

    pub fn free_some_bar(&mut self) -> u8 {
        // Get the oldest by looking for the one with the smallest dep
        let mut bar = 0;
        for b in 1..self.num_bars {
            if self.bar_dep[usize::from(b)] < self.bar_dep[usize::from(bar)] {
                bar = b;
            }
        }
        self.free_bar(bar);
        bar
    }

    pub fn get_bar_for_dep(&self, dep: usize) -> Option<u8> {
        (0..self.num_bars).find(|&bar| self.bar_dep[usize::from(bar)] == dep)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct TexQueueSimulationEntry {
    min_pos: u8,
}

impl TexQueueSimulationEntry {
    const INVALID: Self = Self { min_pos: u8::MAX };

    // First element on the queue
    const FIRST: Self = Self { min_pos: 0 };

    fn is_valid(&self) -> bool {
        if *self == Self::INVALID {
            false
        } else {
            debug_assert!(self.min_pos <= OpTexDepBar::MAX_TEXTURES_LEFT);
            true
        }
    }

    fn push(&mut self) {
        if self.is_valid() {
            self.min_pos += 1;
        }
    }

    const fn flush_after(&mut self, pos: u8) -> bool {
        if self.min_pos < pos {
            true
        } else {
            // This entry is either invalid or higher than the cull level
            *self = Self::INVALID;
            false
        }
    }

    fn merge(&mut self, other: &Self) {
        self.min_pos = self.min_pos.min(other.min_pos);
    }
}

/// Simulate the state of a register in the queue, in buckets of 4
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct TexQueueSimulationBucket {
    entries: [TexQueueSimulationEntry; 4],
}

impl TexQueueSimulationBucket {
    const EMPTY: Self = Self {
        entries: [TexQueueSimulationEntry::INVALID; 4],
    };

    fn min_queue_position(&self, range: Range<usize>) -> Option<u8> {
        self.entries[range]
            .iter()
            .filter(|x| x.is_valid())
            .map(|x| x.min_pos)
            .min()
    }

    fn set_as_first(&mut self, range: Range<usize>) {
        for i in range {
            debug_assert!(!self.entries[i].is_valid());
            self.entries[i] = TexQueueSimulationEntry::FIRST;
        }
    }

    fn push(&mut self) {
        for entry in &mut self.entries {
            entry.push();
        }
    }

    fn flush_after(&mut self, pos: u8) -> bool {
        debug_assert!(pos <= OpTexDepBar::MAX_TEXTURES_LEFT);

        let mut retain = false;
        for x in &mut self.entries {
            retain |= x.flush_after(pos);
        }
        retain
    }

    fn merge(&mut self, other: &Self) {
        for (x, y) in self.entries.iter_mut().zip(other.entries.iter()) {
            x.merge(y);
        }
    }
}

/// This state simulates the texture queue for each destination.
///
/// For example, at the start the queue is always empty, but if we encounter a
/// tex operation that writes in r4..r8, that is pushed on the queue at
/// position 0.  If we encounter another tex operation that only writes r5,
/// that will be pushed at position 0 and the old tex instruction will be in
/// position 1.  This data-structure keeps track of the position of the queue
/// for each destination register present in the queue, push operations
/// correspond to new texture instructions, while flush operations correspond to
/// the usage of registers which may still be on the queue.
///
/// Since all Kepler texture operations use at most 4 registers, and many
/// instruction use more than one destination at a time, we group registers in
/// buckets of 4.  With this optimization each RegRef only accesses a single
/// bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexQueueSimulationState {
    /// Min position of the destination register in the queue,
    /// in buckets of 4 (indexed by register_index / 4).
    queue_pos: FxHashMap<u8, TexQueueSimulationBucket>,
    /// Max length of the queue, needed to check for overflows
    max_queue_len: u8,
}

impl Default for TexQueueSimulationState {
    fn default() -> Self {
        Self::new()
    }
}

impl TexQueueSimulationState {
    pub fn new() -> Self {
        Self {
            queue_pos: FxHashMap::default(),
            max_queue_len: 0,
        }
    }

    /// Translate from RegRef to bucket_index + bucket_range
    #[inline]
    fn reg_ref_to_coords(reg: RegRef) -> (u8, Range<usize>) {
        debug_assert!(reg.base_idx() <= u8::MAX.into());
        let idx = reg.base_idx() as u8 / 4;
        let sub = (reg.base_idx() % 4) as usize;

        let range = sub..(sub + reg.comps() as usize);
        assert!(range.end <= 4);
        (idx, range)
    }

    fn min_queue_position(&self, reg: RegRef) -> Option<u8> {
        let (idx, range) = Self::reg_ref_to_coords(reg);

        self.queue_pos
            .get(&idx)
            .and_then(|x| x.min_queue_position(range))
    }

    const fn is_queue_full(&self) -> bool {
        // MAX_TEXTURES_LEFT describes the maximum number encodable
        // in the texdepbar, but the queue must have an element more.
        self.max_queue_len > OpTexDepBar::MAX_TEXTURES_LEFT
    }

    /// Flush every element whose position >= pos
    ///
    /// Effectively simulates the execution of a `texdepbar pos`
    fn flush_after(&mut self, pos: u8) {
        self.max_queue_len = self.max_queue_len.min(pos);
        self.queue_pos.retain(|_, v| v.flush_after(pos));
    }

    pub fn push(&mut self, reg: RegRef) -> Option<u8> {
        // Assert we are not on the queue
        debug_assert!(self.min_queue_position(reg).is_none());

        // Check that the push operation does not overflow the queue,
        // if it does, we must insert a barrier
        let mut tex_bar = None;
        if self.is_queue_full() {
            // The queue is full, there are 64 in-flight tex-ops.
            // make space by making removing 1 texture.
            tex_bar = Some(OpTexDepBar::MAX_TEXTURES_LEFT);
            self.flush_after(OpTexDepBar::MAX_TEXTURES_LEFT);
            // Now the queue is not full anymore
            debug_assert!(!self.is_queue_full());
        }

        self.max_queue_len += 1;
        // Every entry is pushed by 1
        for x in self.queue_pos.values_mut() {
            x.push();
        }

        // Put us on the queue as first
        let (idx, range) = Self::reg_ref_to_coords(reg);
        self.queue_pos
            .entry(idx)
            .or_insert(TexQueueSimulationBucket::EMPTY)
            .set_as_first(range);

        tex_bar
    }

    pub fn flush(&mut self, reg: RegRef) -> Option<u8> {
        let queue_pos = self.min_queue_position(reg);

        let Some(queue_pos) = queue_pos else {
            return None; // Not in queue
        };

        // Cut the queue
        self.flush_after(queue_pos);
        debug_assert!(self.min_queue_position(reg).is_none());

        Some(queue_pos)
    }

    pub fn merge(&mut self, other: &Self) {
        self.max_queue_len = self.max_queue_len.max(other.max_queue_len);
        for (key, y) in &other.queue_pos {
            let x = self
                .queue_pos
                .entry(*key)
                .or_insert(TexQueueSimulationBucket::EMPTY);
            x.merge(y);
        }
    }

    /// Simulates the execution of an instruction and returns the
    /// barrier level needed.
    pub fn visit_instr(&mut self, instr: &Instr) -> Option<u8> {
        // Flush register reads and writes
        // (avoid write-after-write and read-after-write hazards)
        // Compute the minimum required flush level (for barriers)
        let flush_level = if !self.queue_pos.is_empty() {
            let src_refs = instr.srcs().iter().filter_map(|x| x.reference.as_reg());
            let dst_refs = instr.dsts().iter().filter_map(|x| x.as_reg());

            src_refs
                .chain(dst_refs)
                .filter_map(|reg_ref| self.flush(*reg_ref))
                .reduce(|a, b| a.min(b))
        } else {
            // The queue is empty, no need to check the instruction
            None
        };

        // Push registers (if we are a tex instruction)
        // We might need to insert a barrier if the queue is full
        let push_level = if instr_needs_texbar(instr) {
            let dst = instr.dsts()[0]
                .as_reg()
                .expect("tex instruction must have register destination");
            self.push(*dst)
        } else {
            None
        };

        // If the flush needs a barrier, the queue will not be full,
        // therefore the push will not need a barrier.
        debug_assert!(flush_level.is_none() || push_level.is_none());
        flush_level.or(push_level)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct RegOrigin {
    pub(super) loc: InstrIdx,
    // Index of the src (for reads) or dst (for writes) in the instruction.
    pub(super) src_dst_idx: u16,
}

impl Default for RegOrigin {
    fn default() -> Self {
        // Lower bound
        Self {
            loc: InstrIdx::new(0, 0),
            src_dst_idx: 0,
        }
    }
}

// Delay accumulated from the blocks it passed, used to check for cross-block hazards.
pub(super) type AccumulatedDelay = u8;
pub(super) type DelayRegTracker = SparseRegTracker<RegUseMap<RegOrigin, AccumulatedDelay>>;

pub(super) struct BlockDelayScheduler<'a> {
    pub(super) sm: &'a dyn ShaderModel,
    pub(super) f: &'a Function,
    // Map from barrier to last waited cycle
    pub(super) bars: [u32; 6],
    // Current cycle count until end-of-block.
    pub(super) current_cycle: u32,
    // Map from idx (block, instr) to block-relative cycle
    pub(super) instr_cycles: &'a mut Vec<Vec<u32>>,
}

impl BlockDelayScheduler<'_> {
    /// Compute the starting cycle for an instruction to avoid a dependency hazard.
    fn dependency_to_cycle(
        &self,
        curr_loc: InstrIdx,      // Location of the current instruction
        reg: &RegOrigin,         // Register and location of instruction that will be executed later
        delay: AccumulatedDelay, // Delay between the end of the current block and the later instruction
        latency: u32,            // Latency between current and later instruction
    ) -> u32 {
        debug_assert!(latency <= self.sm.latency_upper_bound());

        let same_block =
            reg.loc.block_idx == curr_loc.block_idx && reg.loc.instr_idx > curr_loc.instr_idx;

        if same_block {
            // Created this transfer pass
            self.instr_cycles[reg.loc.block_idx as usize][reg.loc.instr_idx as usize] + latency
        } else {
            // Remember that cycles are always counted from the end of a block.
            // The next instruction happens after `delay` cycles after the
            // current block is complete, so it is effectively executed at cycle
            // `0 - delay`, adding the latency we get `latency - delay`
            // Underflow means that the instruction is already done (delay > latency).
            latency.saturating_sub(delay.into())
        }
    }

    pub(super) fn process_instr(&mut self, loc: InstrIdx, reg_uses: &mut DelayRegTracker) {
        let instr = &self.f[loc];

        let mut min_start = self.current_cycle + self.sm.exec_latency(&instr.op);

        // Wait on rd/wr barriers
        if let Some(bar) = instr.deps.rd_bar() {
            min_start = max(min_start, self.bars[usize::from(bar)] + 2);
        }
        if let Some(bar) = instr.deps.wr_bar() {
            min_start = max(min_start, self.bars[usize::from(bar)] + 2);
        }

        reg_uses.for_each_instr_dst_mut(instr, |i, u| {
            for (orig, delay) in u.iter_writes() {
                let l = self.sm.waw_latency(
                    &instr.op,
                    i,
                    !instr.pred.predicate.is_none(),
                    &self.f[orig.loc].op,
                    orig.src_dst_idx as usize,
                );
                let s = self.dependency_to_cycle(loc, orig, *delay, l);
                min_start = max(min_start, s);
            }
            for (orig, delay) in u.iter_reads() {
                let l = if orig.src_dst_idx == u16::MAX {
                    self.sm.paw_latency(&instr.op, i)
                } else {
                    self.sm.raw_latency(
                        &instr.op,
                        i,
                        &self.f[orig.loc].op,
                        orig.src_dst_idx as usize,
                    )
                };
                let s = self.dependency_to_cycle(loc, orig, *delay, l);
                min_start = max(min_start, s);
            }

            u.set_write(
                RegOrigin {
                    loc,
                    src_dst_idx: i as u16,
                },
                0,
            );
        });

        reg_uses.for_each_instr_pred_mut(instr, |c| {
            // WaP does not exist
            c.add_read(
                RegOrigin {
                    loc,
                    src_dst_idx: u16::MAX,
                },
                0,
            );
        });
        reg_uses.for_each_instr_src_mut(instr, |i, u| {
            for (orig, delay) in u.iter_writes() {
                let l = self.sm.war_latency(
                    &instr.op,
                    i,
                    &self.f[orig.loc].op,
                    orig.src_dst_idx as usize,
                );
                let s = self.dependency_to_cycle(loc, orig, *delay, l);
                min_start = max(min_start, s);
            }

            u.add_read(
                RegOrigin {
                    loc,
                    src_dst_idx: i as u16,
                },
                0,
            );
        });

        self.instr_cycles[loc.block_idx as usize][loc.instr_idx as usize] = min_start;

        // Kepler A membar conflicts with predicate writes
        if self.sm.is_kepler_a() && matches!(&instr.op, Op::MemBar(_)) {
            let read_origin = RegOrigin {
                loc,
                src_dst_idx: u16::MAX,
            };
            reg_uses.for_each_pred(|c| {
                c.add_read(read_origin, 0);
            });
            reg_uses.for_each_carry(|c| {
                c.add_read(read_origin, 0);
            });
        }

        // "Issue" barriers other instructions will wait on.
        for (bar, c) in self.bars.iter_mut().enumerate() {
            if instr.deps.wt_bar_mask & (1 << bar) != 0 {
                *c = min_start;
            }
        }

        self.current_cycle = min_start;
    }
}

const fn instr_needs_texbar(instr: &Instr) -> bool {
    matches!(
        instr.op,
        Op::Tex(_) | Op::Tld(_) | Op::Tmml(_) | Op::Tld4(_) | Op::Txd(_) | Op::Txq(_)
    )
}
