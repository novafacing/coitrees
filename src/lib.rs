
//! # COITrees
//! `coitrees` implements a fast static interval tree data structure with genomic
//! data in mind.
//!

use std::fmt::Debug;
use std::collections::{Bound, BTreeMap};
use std::option::Option;
use std::time::Instant;
use std::cmp::{min, max};
use std::mem::transmute;


use std::arch::x86_64::{
    __m256i,
    _mm256_cmpgt_epi32,
    _mm256_movemask_epi8,
    _mm256_set1_epi32,
    _mm256_set_epi32,
    _mm256_and_si256,
};


type i32x8 = __m256i;


const DEFAULT_SPARSITY: f64 = 30.0;
const MIN_FINAL_SEQ_LEN: usize = 2;


#[derive(Clone, Copy, Debug)]
pub struct Interval<I, T> where I: Clone, T: Clone {
    pub first: I,
    pub last: I,
    pub metadata: T
}



// 8 intervals packed into AVX registers
// TODO: We could experiment with packing even more intervals (i.e., multiple
// i32x8's) into a chunk
#[derive(Copy, Clone, Debug)]
struct IntervalChunk {
    firsts: i32x8,
    lasts: i32x8,

    min_first: i32,
    max_last: i32,

    metadata_idx: [usize; 8],
    chunk_num: usize
}


impl IntervalChunk {
    fn new(intervals: &[Interval<i32, usize>; 8]) -> IntervalChunk {
        unsafe {
            let mut min_first = intervals[0].first;
            let mut max_last = intervals[0].last;
            for interval in &intervals[1..] {
                if interval.first != i32::MIN+1 {
                    min_first = min(min_first, interval.first);
                }
                if interval.last != i32::MIN {
                    max_last = max(max_last, interval.last);
                }
            }

            // the firsts and lasts are adjust by 1 here because AVX has an
            // instruction for > but not >=.
            let firsts =_mm256_set_epi32(
                intervals[0].first - 1,
                intervals[1].first - 1,
                intervals[2].first - 1,
                intervals[3].first - 1,
                intervals[4].first - 1,
                intervals[5].first - 1,
                intervals[6].first - 1,
                intervals[7].first - 1);

            let lasts =_mm256_set_epi32(
                intervals[0].last + 1,
                intervals[1].last + 1,
                intervals[2].last + 1,
                intervals[3].last + 1,
                intervals[4].last + 1,
                intervals[5].last + 1,
                intervals[6].last + 1,
                intervals[7].last + 1);

            let metadata_idx: [usize; 8] = [
                intervals[0].metadata,
                intervals[1].metadata,
                intervals[2].metadata,
                intervals[3].metadata,
                intervals[4].metadata,
                intervals[5].metadata,
                intervals[6].metadata,
                intervals[7].metadata ];

            return Self {
                firsts: firsts,
                lasts: lasts,
                min_first: min_first,
                max_last: max_last,
                metadata_idx: metadata_idx,
                chunk_num: usize::MAX,
            }
        }
    }


    #[inline(always)]
    fn query_count_chunk(&self, query_first: i32x8, query_last: i32x8) -> usize {
        unsafe {
            // let cmp1 = _mm256_cmpgt_epi32(query_last, self.firsts);
            // let cmp1_32 = _mm256_movemask_epi8(cmp1);
            // let cmp2 = _mm256_cmpgt_epi32(self.lasts, query_first);
            // let cmp2_32 = _mm256_movemask_epi8(cmp2);
            // let count = (cmp1_32 & cmp2_32).count_ones() / 4;

            let cmp1 = _mm256_cmpgt_epi32(query_last, self.firsts);
            let cmp2 = _mm256_cmpgt_epi32(self.lasts, query_first);
            let cmp = _mm256_movemask_epi8(_mm256_and_si256(cmp1, cmp2));
            let count = cmp.count_ones() / 4;

            return count as usize;
        }
    }


    #[inline(always)]
    fn query_chunk(&self, query_first: i32x8, query_last: i32x8) -> (u32, u32) {
        unsafe {
            let cmp1 = _mm256_cmpgt_epi32(query_last, self.firsts);
            let cmp1_32 = _mm256_movemask_epi8(cmp1);

            let cmp2 = _mm256_cmpgt_epi32(self.lasts, query_first);
            let cmp2_32 = _mm256_movemask_epi8(cmp2);

            return (transmute(cmp1_32), transmute(cmp1_32));
        }
    }
}


// fn chunk_intervals<T>(intervals: &Vec<Interval<i32, T>>) -> Vec<IntervalChunk>> {

// }


// struct SearchableIntervals {
//     chunks: Vec<IntervalChunk>,

//     chunk_max_leaf_nums: BTreeSet<u32>,
//     nearest_chunk_max_leaf_num: u32,

//     // buffer intervals before packing them into a IntervalChunk
//     buf: [Interval<i32, u32>; 8],
//     bufoffset: usize
// }


// impl SearchableIntervals {
//     fn with_capacity(capacity: usize) -> Self {
//         return Self {
//             chunks: Vec::with_capacity(capacity / 8 + (capacity % 8 != 0) as usize),
//             chunk_max_leaf_nums: BTreeSet::new(),
//             nearest_chunk_max_leaf_num: u32::MAX,
//             buf: [Interval{first: i32::MAX, last: i32::MIN, metadata: 0}; 8],
//             bufoffset: 0,
//         }
//     }

//     // Add a new interval to the back of the searchable intervals.
//     //
//     // We have to take care to produce chunks that don't have a range of leaf
//     // numbers overlapping the maximum leaf number of any prior chunk. This
//     // is so we can process chunks one at a time without having to chuck maximum
//     // leaf number at each step.
//     //
//     // TODO: What can we do with metadata to deal with padding?
//     // I think I really just have to store an index with each
//     // interval that points to metadata
//     fn push(&mut self, interval: Interval<i32, u32>) {
//         if self.bufoffset > 0 {
//             let last_interval = self.buf[self.bufoffset - 1];
//             if interval.metadata > self.nearest_chunk_max_leaf_num || interval.metadata <= last_interval.metadata {
//                 self.dump_chunk();
//             }
//         }

//         if self.bufoffset == 0 {
//             if let Some(nearest_chunk_max_leaf_num) = self.chunk_max_leaf_nums.range(
//                     (Bound::Included(interval.metadata), Bound::Unbounded)).next() {
//                 self.nearest_chunk_max_leaf_num = *nearest_chunk_max_leaf_num;
//             } else {
//                 self.nearest_chunk_max_leaf_num = u32::MAX;
//             }
//         }

//         assert!(self.bufoffset < 8);
//         self.buf[self.bufoffset] = interval;
//         self.bufoffset += 1;
//         if self.bufoffset == 8 {
//             self.dump_chunk();
//         }
//     }


//     fn dump_chunk(&mut self) {
//         if self.bufoffset == 0 {
//             return;
//         }

//         while self.bufoffset < 8 {
//             self.buf[self.bufoffset] = Interval{
//                 first: i32::MIN+1,
//                 last: i32::MIN,
//                 metadata: u32::MIN,
//             };

//             self.bufoffset += 1;
//         }

//         self.chunks.push(IntervalChunk::new(&self.buf));
//         self.chunk_max_leaf_nums.insert(self.chunks.last().unwrap().max_leaf_num);
//         self.bufoffset = 0;
//     }


//     fn len(&self) -> usize {
//         return self.chunks.len() * 8 + self.bufoffset;
//     }
// }


/// COITree data structure. A representation of a static set of intervals with
/// associated metadata, enabling fast overlap and coverage queries.
pub struct COITree<T> {

    // index into `intervals` according to query `last`
    // TODO: since this is static may be worth switching to a sorted list of
    // pairs and just doing interpolation search.
    index: BTreeMap<i32, usize>,

    // intervals arranged to facilitate linear searches
    // intervals: Vec<Interval<I, u32>>,
    searchable_intervals: Vec<IntervalChunk>,

    // metadata associated with each interval in intervals
    metadata: Vec<T>
}


#[derive(Copy, Clone, Debug)]
pub struct SurplusTreeNode {
    // how many more points could lie below the y boundary before
    // the query region represented by this tree becomes sparse
    surplus: f64,

    // minimum surplus of any prefix in the subtree rooted at this node.
    // this is called "tolerance" in the Arge paper
    min_prefix_surplus: f64,

    // number of leaf nodes in the subtree not marked as dead
    live_nodes: usize,

    // if this is a leaf node, the corresponding index into the
    // chunked_intervals array otherwise usize::MAX
    intervals_index: usize,
}


impl Default for SurplusTreeNode {
    fn default() -> Self {
        return SurplusTreeNode {
            surplus: f64::NAN,
            min_prefix_surplus: f64::NAN,
            live_nodes: 0,
            intervals_index: usize::MAX
        };
    }
}


/// Data structure used when constructing the COITree to keep track of the
/// "surplus" of each prefix which let's us find for the current y boundary
/// whether there are any sparse queries and where the maximum x value is
/// for that query.
pub struct SurplusTree {
    sparsity: f64,

    // these are stored in binary heap order
    nodes: Vec<SurplusTreeNode>,

    // reverse direction: map node index to leaf node index
    index_to_leaf: Vec<usize>,
}


// Function for traversing from one leaf node index to the next.
fn next_live_leaf(nodes: &Vec<SurplusTreeNode>, mut i: usize) -> Option<usize> {
    let num_nodes = nodes.len();

    let mut left = 2*i+1;
    let mut right = 2*i+2;

    if left < num_nodes {
        // internal node: climb down until we find a live leaf
        if nodes[i].live_nodes == 0 {
            return None;
        }

        while left < num_nodes {
            if nodes[left].live_nodes > 0 {
                i = left;
            } else {
                i = right;
            }

            left = 2*i+1;
            right = 2*i+2;
        }
    } else {
        // leaf node: climb up until we are someone's left child and right
        // is live

        loop {
            if i == 0 {
                return None;
            }

            let parent = (i-1)/2;
            let parent_left = 2*parent+1;
            let parent_right = 2*parent+2;

            debug_assert!(i == parent_left || i == parent_right);
            if i == parent_left && nodes[parent_right].live_nodes > 0 {
                i = parent_right;
                break;
            }

            i = parent;
        }

        // now climb down and find a live node
        left = 2*i+1;
        right = 2*i+2;
        while left < num_nodes {
            if nodes[left].live_nodes > 0 {
                i = left;
            } else {
                i = right;
            }

            left = 2*i+1;
            right = 2*i+2;
        }
    }

    return Some(i);
}


impl SurplusTree where {

    // Constructs a surplus tree for the chunked intervals, but also sets the
    // chunk_num field for each chunk, which we know after x sorting.
    fn new(chunked_intervals: &mut Vec<IntervalChunk>, sparsity: f64) -> Self {
        let n = chunked_intervals.len();

        // permutation that puts (y-sorted) nodes in x-sorted order.
        let mut xperm: Vec<usize> = (0..n).collect();
        xperm.sort_by_key(|i| chunked_intervals[*i].min_first);
        for (i, j) in xperm.iter().enumerate() {
            chunked_intervals[*j].chunk_num = i+1;
        }

        let num_nodes = 2*n-1;
        let mut nodes = vec![SurplusTreeNode::default(); num_nodes];
        let mut index_to_leaf: Vec<usize> = vec![usize::default(); n];

        // go up the tree setting internal node values
        let mut i = nodes.len() - 1;
        loop {
            let left = 2*i+1;
            let right = 2*i+2;

            // is a leaf node
            if left < nodes.len() {
                nodes[i].live_nodes = nodes[left].live_nodes + nodes[right].live_nodes;
            } else {
                nodes[i].live_nodes = 1;
            }

            nodes[i].min_prefix_surplus = sparsity - 1.0;
            nodes[i].surplus = (sparsity - 1.0) * nodes[i].live_nodes as f64;

            if i == 0 {
                break;
            } else {
                i -= 1;
            }
        }

        // iterate through leaves, initializing leaf node values
        let mut i = 0;
        let mut leaf_count = 0;
        while let Some(j) = next_live_leaf(&nodes, i) {
            let idx = xperm[leaf_count];
            nodes[j].intervals_index = idx;
            leaf_count += 1;
            i = j;
            assert_eq!(chunked_intervals[nodes[j].intervals_index].chunk_num, leaf_count);
        }

        // reverse the map
        for (i, node) in nodes.iter().enumerate() {
            if node.intervals_index != usize::MAX {
                index_to_leaf[node.intervals_index] = i;
            }
        }

        return Self {
            sparsity: sparsity,
            nodes: nodes,
            index_to_leaf: index_to_leaf,
        };
    }

    fn len(&self) -> usize {
        return self.nodes.len();
    }


    // Climb up to the root from node `j`, setting each ancestors `min_prefix_surplus`
    // and calling `visit` on each ancestor. This needs to be as fast as possible,
    // so we do unsafe indexing.
    fn update_ancestors<F>(&mut self, j: usize, mut visit: F)
            where F: FnMut(&mut SurplusTreeNode) {

        assert!(j < self.nodes.len());
        let mut root = j;
        while root > 0 {
            root = (root-1)/2;

            let left = 2*root+1;
            let right = 2*root+2;

            let (left_min_prefix_surplus, left_surplus) = {
                let node_left = unsafe { self.nodes.get_unchecked(left) };
                (node_left.min_prefix_surplus, node_left.surplus)
            };

            let right_min_prefix_surplus = {
                let node_right = unsafe { self.nodes.get_unchecked(right) };
                node_right.min_prefix_surplus
            };

            let node_root = unsafe { self.nodes.get_unchecked_mut(root) };

            // node_root.min_prefix_surplus = left_min_prefix_surplus.min(
            //     left_surplus + right_min_prefix_surplus);

            let l = left_min_prefix_surplus;
            let r = left_surplus + right_min_prefix_surplus;
            node_root.min_prefix_surplus = if l < r { l } else { r };

            visit(node_root);
        }
    }

    // Called on nodes below the sweep line when they are also to the left of
    // the maximum sparse x query point (and thus no longer in S_i)
    fn set_node_dead(&mut self, i: usize) {
        let j = self.index_to_leaf[i];

        {
            let node = &mut self.nodes[j];
            // disregard any prefix that ends on this node
            node.min_prefix_surplus = f64::INFINITY;
            node.surplus += 1.0;
            node.live_nodes -= 1;

        }
        self.update_ancestors(j, |node| {
            node.surplus += 1.0;
            node.live_nodes -= 1;
        });
    }

    // Called on nodes when they fall below the sweep line
    fn set_node_useless(&mut self, i: usize) {
        let j = self.index_to_leaf[i];

        {
            let node = &mut self.nodes[j];
            // disregard any prefix that ends on this node
            node.min_prefix_surplus = f64::INFINITY;
            node.surplus -= self.sparsity;
        }

        let sparsity = self.sparsity;
        self.update_ancestors(j, |node| {
            node.surplus -= sparsity;
        });
    }


    fn num_live_nodes(&self) -> usize {
        return self.nodes[0].live_nodes;
    }


    fn last_live_leaf(&self) -> Option<usize> {
        let mut i = 0;

        if self.nodes[i].live_nodes == 0 {
            return None;
        }

        let num_nodes = self.len();
        let mut left = 2*i+1;
        let mut right = 2*i+2;

        while left < num_nodes {
            if self.nodes[right].live_nodes > 0 {
                i = right;
            } else {
                i = left;
            }

            left = 2*i+1;
            right = 2*i+2;
        }

        return Some(i);
    }


    // Call the function `visit` for each live node in the prefix up to end_idx,
    // inclusively.
    fn map_prefix<F>(&mut self, end_idx: usize, mut visit: F)
            where F: FnMut(&mut Self, usize) {

        let mut i = 0;
        loop {
            if let Some(j) = next_live_leaf(&self.nodes, i) {
                let idx = self.nodes[j].intervals_index;
                visit(self, idx);
                if j == end_idx {
                    break;
                }
                i = j;
            } else {
                break;
            }
        }
    }


    // If there is a sparse query, return the index with the corresponding
    // maximum x boundary..
    // If there is no sparse query, return None.
    fn find_sparse_query_prefix(&self) -> Option<usize> {
        if self.nodes[0].min_prefix_surplus >= 0.0 {
            return None;
        } else {
            let n = self.len();
            let mut j = 0;
            let mut prefix_surplus = 0.0;
            loop {
                let left = 2*j+1;
                let right = 2*j+2;
                debug_assert!((left < n) == (right < n));

                if left < n {
                    let right_prefix_surplus =
                        prefix_surplus +
                        self.nodes[left].surplus +
                        self.nodes[right].min_prefix_surplus;

                    if right_prefix_surplus < 0.0 {
                        prefix_surplus += self.nodes[left].surplus;
                        j = right;
                    } else {
                        j = left;
                    }
                } else {
                    prefix_surplus += self.nodes[j].surplus;
                    break;
                }
            }

            // TODO: This doesn't really resolve the issue. Any point in keeping it?
            // find the furthest leaf node that shares the same `first` value
            // this is to avoid splitting up points that share the same values,
            // which leads to issues.
            // while let Some(k) = self.next_live_leaf(j) {
            //     if intervals[self.leaf_to_index[&j].0].first == intervals[self.leaf_to_index[&k].0].first {
            //         j = k;
            //     } else {
            //         break;
            //     }
            // }

            assert!(prefix_surplus < 0.0);
            return Some(j);
        }
    }
}


impl<T> COITree<T> {
    pub fn new(intervals: Vec<Interval<i32, T>>) -> COITree<T>
            where T: Copy {
        return Self::with_sparsity(intervals, DEFAULT_SPARSITY);
    }


    fn chunk_intervals(intervals: &mut Vec<Interval<i32, T>>) -> (Vec<IntervalChunk>, Vec<T>)
            where T: Copy {
        let n = intervals.len();
        let num_chunks = (n / 8) + (n % 8 != 0) as usize;
        let mut chunked_intervals: Vec<IntervalChunk> = Vec::with_capacity(num_chunks);
        let mut metadata: Vec<T> = Vec::with_capacity(n);

        // chunk initializer
        let mut chunk_init: [Interval<i32, usize>; 8] =
            [Interval{first: i32::MAX, last: i32::MIN, metadata: usize::MAX}; 8];

        for (i, group) in intervals.chunks(8).enumerate() {
            for (j, interval) in group.iter().enumerate() {
                chunk_init[j] = Interval{
                    first: interval.first,
                    last: interval.last,
                    metadata: metadata.len()
                };
                metadata.push(interval.metadata);
            }

            // pad the end if needed
            for j in group.len()..8 {
                chunk_init[j] = Interval {
                    first: i32::MIN+1,
                    last: i32::MIN,
                    metadata: usize::MAX
                };
            }

            chunked_intervals.push(
                IntervalChunk::new(&chunk_init));
        }

        return (chunked_intervals, metadata);
    }


    pub fn with_sparsity(mut intervals: Vec<Interval<i32, T>>, sparsity: f64) -> COITree<T>
            where T: Copy {
        assert!(sparsity > 1.0);

        if intervals.is_empty() {
            return Self{
                index: BTreeMap::new(),
                searchable_intervals: Vec::new(),
                metadata: Vec::new()
            }
        }

        intervals.sort_unstable_by_key(|i| i.last);

        // chunk intervals
        let (mut chunked_intervals, metadata) = Self::chunk_intervals(&mut intervals);
        let num_chunks = chunked_intervals.len();
        dbg!(intervals.len());
        dbg!(num_chunks);

        let max_size: usize = ((sparsity / (sparsity - 1.0)) * (num_chunks as f64)).ceil() as usize;
        dbg!(max_size);

        let mut searchable_intervals: Vec<IntervalChunk> = Vec::with_capacity(max_size);
        let mut index: BTreeMap<i32, usize> = BTreeMap::new();
        let mut surplus_tree = SurplusTree::new(&mut chunked_intervals, sparsity);

        // searchable intervals stores an extra integer to disambiguate and
        // avoid counting the same hits more than once.

        let mut boundary = i32::MIN;

        let now = Instant::now();
        let mut i = 0;
        while i <= num_chunks && surplus_tree.num_live_nodes() > 0 {
            let max_end_opt = if num_chunks - i <= MIN_FINAL_SEQ_LEN {
                i = num_chunks-1;
                surplus_tree.last_live_leaf()
            } else {
                surplus_tree.find_sparse_query_prefix()
            };

            if let Some(max_end) = max_end_opt {
                let last_boundary = boundary;
                index.insert(last_boundary, searchable_intervals.len());
                boundary = chunked_intervals[i].max_last;

                surplus_tree.map_prefix(max_end, |tree, idx| {
                    let interval_chunk = &chunked_intervals[idx];
                    searchable_intervals.push(*interval_chunk);

                    if interval_chunk.max_last < boundary {
                        tree.set_node_dead(idx);
                    }
                });
            }

            if num_chunks -i <= MIN_FINAL_SEQ_LEN {
                break;
            }

            surplus_tree.set_node_useless(i);
            i += 1;

            // make sure the next value in distinct, otherwise we can choose
            // a boundary between equal values which can break things
            while i < num_chunks && intervals[i].last == intervals[i-1].last {
                surplus_tree.set_node_useless(i);
                i += 1;
            }
        }
        eprintln!("main construction loop: {}", now.elapsed().as_millis() as f64 / 1000.0);

        dbg!(searchable_intervals.len());
        dbg!(index.len());
        // dbg!(&index);

        return Self{
            index: index,
            searchable_intervals: searchable_intervals,
            metadata: metadata
        };
    }


    #[inline(always)]
    fn find_search_start(&self, first: i32) -> Option<usize> {
        if let Some((_, i)) = self.index.range((Bound::Unbounded, Bound::Included(first))).next_back() {
            return Some(*i);
        } else {
            return None;
        }
    }

    pub fn query_count(&self, first: i32, last: i32) -> usize {
        // let mut misses = 0;
        let mut count = 0;
        if let Some(search_start) = self.find_search_start(first) {
            let (first_vec, last_vec) = unsafe {
                (_mm256_set1_epi32(first), _mm256_set1_epi32(last)) };

            let mut last_chunk_num = 0;

            let mut searched_chunks = 0;
            for chunk in &self.searchable_intervals[search_start..] {
                searched_chunks += 1;

                if last < chunk.min_first {
                    break;
                }

                if chunk.max_last < first || chunk.chunk_num <= last_chunk_num {
                    continue;
                }
                last_chunk_num = chunk.chunk_num;

                let chunk_count =  chunk.query_count_chunk(first_vec, last_vec);
                count += chunk_count;
            }
        }

        return count;
    }
}
