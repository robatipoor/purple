/*
  Copyright 2018 The Purple Library Authors
  This file is part of the Purple Library.

  The Purple Library is free software: you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  The Purple Library is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with the Purple Library. If not, see <http://www.gnu.org/licenses/>.
*/

use crate::block::Block;
use crate::pending_state::{Branches, PendingState};
use crate::orphan_state::{OrphanState, OrphanType};
use crate::chain::{Chain, ChainErr};
use crate::easy_chain::chain::EasyChainRef;
use crate::hard_chain::block::HardBlock;
use crate::iterators::hard::HardBlockIterator;
use bin_tools::*;
use crypto::Hash;
use elastic_array::ElasticArray128;
use hashbrown::{HashMap, HashSet};
use hashdb::HashDB;
use lru::LruCache;
use parking_lot::{RwLock, Mutex};
use persistence::PersistentDb;
use std::sync::Arc;
use lazy_static::*;

/// Size of the block cache.
const BLOCK_CACHE_SIZE: usize = 20;

/// Maximum orphans allowed.
const MAX_ORPHANS: usize = 10;

/// Blocks with height below the canonical height minus 
/// this number will be rejected.
const MIN_HEIGHT: u64 = 10;

/// Blocks with height below the canonical height minus
/// this number will be rejected.
const MAX_HEIGHT: u64 = 10;

lazy_static! {
    /// Atomic reference count to hard chain genesis block
    static ref GENESIS_RC: Arc<HardBlock> = { 
        let mut block = HardBlock::genesis();
        block.compute_hash();

        Arc::new(block) 
    };
    
    /// Canonical tip block key
    static ref TIP_KEY: Hash = { crypto::hash_slice(b"canonical_tip") };

    /// The key to the canonical height of the chain
    static ref CANONICAL_HEIGHT_KEY: Hash = { crypto::hash_slice(b"canonical_height") };
}

#[derive(Clone)]
/// Thread-safe reference to an easy chain and its block cache.
pub struct HardChainRef {
    /// Reference to easy chain.
    pub chain: Arc<RwLock<HardChain>>,

    /// Block lookup cache.
    block_cache: Arc<Mutex<LruCache<Hash, Arc<HardBlock>>>>
}

impl HardChainRef {
    pub fn new(chain: Arc<RwLock<HardChain>>) -> HardChainRef {
        HardChainRef {
            chain,
            block_cache: Arc::new(Mutex::new(LruCache::new(BLOCK_CACHE_SIZE)))
        }
    }

    /// Attempts to fetch a block by its hash from the cache
    /// and if it doesn't succeed it then attempts to retrieve
    /// it from the database.
    pub fn query(&self, hash: &Hash) -> Option<Arc<HardBlock>> {
        let cache_result = {
            let mut cache = self.block_cache.lock();

            if let Some(result) = cache.get(hash) {
                Some(result.clone())
            } else {
                None
            }
        };

        if let Some(result) = cache_result {
            Some(result)
        } else {
            let chain_result = {
                let chain = self.chain.read();
            
                if let Some(result) = chain.query(hash) {
                    Some(result)
                } else {
                    None
                }
            };

            if let Some(result) = chain_result {
                let mut cache = self.block_cache.lock();

                if cache.get(hash).is_none() {
                    // Cache result and then return it
                    cache.put(hash.clone(), result.clone());
                }

                Some(result)
            } else {
                None
            }
        }
    }
}

/// The hard chain stores blocks that represent state 
/// changes in the validator pool. A block from the hard chain
/// can be thought of as a function which changes the state of
/// the validator pool.
///
/// From the point of view of the validator pool a
/// block mined on the hard chain represents an
/// injection of:
///
/// 1. An additional amount of events that the whole pool can order.
/// 2. Additional validators.
///
/// The pool cannot start ordering events without a block
/// being mined in the hard chain which states the new
/// validators that will be added (miners of the latest
/// easy chain blocks since that last mined hard block),
/// how many events the pool can order in the next round,
/// and what nodes to retire from the pool.
///
/// At the same time, the next hard block cannot be applied
/// to the pool until the pool has either consumed all of
/// their allocated events or until the pool is deemed to be
/// corrupt.
pub struct HardChain {
    /// Reference to the database storing the `HardChain`.
    db: PersistentDb,

    /// Reference to associated easy chain.
    easy_chain: EasyChainRef,

    /// The current height of the chain.
    height: u64,

    /// The tipmost block in the canonical chain.
    canonical_tip: Arc<HardBlock>,

    /// Cache storing the tip blocks descended from the
    /// canonical chain and excluding the actual canonical
    /// tip block.
    canonical_tips_cache: HashSet<Hash>,

    /// Cache storing the tip blocks of chains that are
    /// disconnected from the canonical chain.
    pending_tips_cache: HashSet<Hash>,

    /// Mapping between disconnected chain head
    /// blocks parent hashes and their own hashes
    /// along with the hashes of the top blocks
    /// from disconnected chains that descend
    /// from the respective head block.
    pending_heads_parents: HashMap<Hash, Branches>,

    /// Memory pool of blocks that are not in the canonical chain.
    orphan_pool: HashMap<Hash, OrphanState<HardBlock>>,
}

impl HardChain {
    pub fn new(mut db_ref: PersistentDb, easy_chain: EasyChainRef) -> HardChain {
        let tip_db_res = db_ref.get(&TIP_KEY);
        let canonical_tip = match tip_db_res.clone() {
            Some(tip) => {
                let mut buf = [0; 32];
                buf.copy_from_slice(&tip);

                let block_bytes = db_ref.get(&Hash(buf)).unwrap();
                Arc::new(HardBlock::from_bytes(&block_bytes).unwrap())
            }
            None => {
                HardChain::genesis()
            }
        };

        let height = match db_ref.get(&CANONICAL_HEIGHT_KEY) {
            Some(height) => decode_be_u64!(&height).unwrap(),
            None => {
                if tip_db_res.is_none() {
                    // Set 0 height
                    db_ref.emplace(
                        CANONICAL_HEIGHT_KEY.clone(),
                        ElasticArray128::<u8>::from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]),
                    );
                }

                0
            }
        };

        let height = height;

        HardChain {
            canonical_tip,
            canonical_tips_cache: HashSet::with_capacity(MAX_ORPHANS),
            pending_heads_parents: HashMap::with_capacity(MAX_ORPHANS),
            pending_tips_cache: HashSet::with_capacity(MAX_ORPHANS),
            orphan_pool: HashMap::with_capacity(MAX_ORPHANS),
            height,
            easy_chain,
            db: db_ref,
        }
    }

    // TODO: Make writes atomic
    fn write_block(&mut self, block: Arc<HardBlock>) {
        let block_hash = block.block_hash().unwrap();

        // Place block in the ledger
        self.db.emplace(
            block_hash.clone(),
            ElasticArray128::<u8>::from_slice(&block.to_bytes()),
        );

        // Set new tip block
        self.canonical_tip = block.clone();
        let mut height = decode_be_u64!(self.db.get(&CANONICAL_HEIGHT_KEY).unwrap()).unwrap();

        // Increment height
        height += 1;

        // Set new height
        self.height = height;

        // Write new height
        let encoded_height = encode_be_u64!(height);
        self.db.emplace(
            CANONICAL_HEIGHT_KEY.clone(),
            ElasticArray128::<u8>::from_slice(&encoded_height),
        );

        // Write block height
        let block_height_key = format!("{}.height", hex::encode(block_hash.to_vec()));
        let block_height_key = crypto::hash_slice(block_height_key.as_bytes());

        self.db.emplace(
            block_height_key,
            ElasticArray128::<u8>::from_slice(&encoded_height)
        );

        // Mark new hard chain tip block in easy chain
        let mut easy_chain = self.easy_chain.chain.write();
        easy_chain.set_hard_canonical_tip(&block.block_hash().unwrap()).unwrap();
    }
}

impl<'a> Chain<'a, HardBlock, HardBlockIterator<'a>> for HardChain {
    fn genesis() -> Arc<HardBlock> {
        GENESIS_RC.clone()
    }

    fn query(&self, hash: &Hash) -> Option<Arc<HardBlock>> {
        if let Some(stored) = self.db.get(hash) {
            // Store to heap
            let heap_stored = Arc::new(HardBlock::from_bytes(&stored).unwrap());
            Some(heap_stored)
        } else {
            None
        }
    }

    fn query_by_height(&self, height: u64) -> Option<Arc<HardBlock>> {
        unimplemented!();
    }

    fn block_height(&self, hash: &Hash) -> Option<u64> {
        unimplemented!();
    }

    fn append_block(&mut self, block: Arc<HardBlock>) -> Result<(), ChainErr> {
        let min_height = if self.height > MIN_HEIGHT {
            self.height - MIN_HEIGHT
        } else {
            1
        };

        if block.height() > self.height + MAX_HEIGHT || block.height() < min_height {
            return Err(ChainErr::BadHeight);
        }

        let tip = &self.canonical_tip;
        let block_hash = block.block_hash().unwrap();

        if let Some(parent_hash) = block.parent_hash() {
            // First attempt to place the block after the 
            // tip canonical block.
            if parent_hash == tip.block_hash().unwrap() {
                // The height must be equal to that of the parent plus one
                if block.height() != self.height + 1 {
                    return Err(ChainErr::BadHeight);
                }

                // Attempt to connect the new canonical tip to pending chains
                let mut block_match = None;

                for (parent, block) in self.pending_heads_parents.iter() {
                    if *parent == block_hash {
                        block_match = Some(block.clone());
                        break;
                    } 
                }

                if let Some(block_match) = block_match {
                    // We need to connect another chain as 
                    // the canonical extension.
                    unimplemented!();
                } else {
                    // Write block to the chain
                    self.write_block(block);
                }

                Ok(())
            } else {
                if self.canonical_tips_cache.contains(&parent_hash) {
                    let parent_height = self.orphan_pool.get(&parent_hash).unwrap().inner().height();

                    // The block height must be equal to that of the parent plus one
                    if block.height() != parent_height + 1 {
                        return Err(ChainErr::BadHeight);
                    }

                    // Attempt to connect disconnected chains to 
                    // the newely appended block.
                    unimplemented!();

                    // Check if the new chain can become canonical
                    unimplemented!();

                    // Place block in the orphan pool
                    self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block.clone(), OrphanType::CanonicalTip));

                    // Remove parent block from cache
                    self.canonical_tips_cache.remove(&parent_hash);

                    // Insert new block in cache
                    self.canonical_tips_cache.insert(block_hash);

                    Ok(())
                } else if self.pending_tips_cache.contains(&parent_hash) {
                    let parent_height = self.orphan_pool.get(&parent_hash).unwrap().inner().height();

                    // The block height must be equal to that of the parent plus one
                    if block.height() != parent_height + 1 {
                        return Err(ChainErr::BadHeight);
                    }

                    // Update parent
                    let parent_entry = self.orphan_pool.get_mut(&parent_hash).unwrap();

                    // Update parent type
                    match parent_entry.orphan_type() {
                        OrphanType::PendingTipHead => {
                            // Make tip in this case
                            parent_entry.set_type(OrphanType::PendingHead);
                        }
                        _ => {
                            parent_entry.set_type(OrphanType::PendingNonTip);
                        }
                    };

                    let mut block_match = None;

                    // Attempt to connect to other pending chains
                    for (parent, state) in self.pending_heads_parents.iter() {
                        if *parent == block_hash {
                            block_match = Some((parent, state.clone()));
                            break;
                        } 
                    }

                    // Connect to chain if we have a match
                    if let Some((block_match, state)) = block_match {
                        // Place block in the orphan pool
                        self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block.clone(), OrphanType::PendingNonTip));

                        let block_match = self.orphan_pool.get_mut(&block_match).unwrap();

                        // Mark as pending non tip
                        block_match.set_type(OrphanType::PendingNonTip);

                        // Merge states
                        unimplemented!();
                    } else {
                        // Insert new block in cache
                        self.pending_tips_cache.insert(block.block_hash().unwrap());

                        // Place block in the orphan pool
                        self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block.clone(), OrphanType::PendingTip));
                    }

                    // Remove parent block from cache
                    self.pending_tips_cache.remove(&parent_hash);

                    Ok(())
                } else if self.pending_heads_parents.get(&block_hash).is_some() {
                    let mut replace_state = false;

                    {
                        let entry = self.pending_heads_parents.get_mut(&block_hash).unwrap();

                        for (child, state) in entry.iter_mut() {
                            // Update child
                            let child_entry = self.orphan_pool.get_mut(child).unwrap();

                            // Update child type
                            match child_entry.orphan_type() {
                                OrphanType::PendingTipHead => {
                                    // Mark as tip in this case
                                    child_entry.set_type(OrphanType::PendingTip);
                                }
                                _ => {
                                    state.remove_tip(child);
                                    child_entry.set_type(OrphanType::PendingNonTip);
                                }
                            };

                            let mut found_match = false;

                            // Attempt to connect to other pending chains
                            for tip_hash in self.pending_tips_cache.iter() {
                                let tip = self.orphan_pool.get_mut(tip_hash).unwrap();

                                if *tip_hash == parent_hash {
                                    // Mark old tip as non tip
                                    tip.set_type(OrphanType::PendingNonTip);
                                    state.remove_tip(tip_hash);
                                    
                                    found_match = true;
                                    break;
                                } 
                            }
                        
                            // Do not cache as head if we connected the 
                            // chain to another tip.
                            if !found_match {
                                // Mark for replacement
                                replace_state = true;

                                // Write block to orphan pool
                                self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block.clone(), OrphanType::PendingHead));
                            } else {
                                // Merge state from connected chain to our state
                                unimplemented!();

                                self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block, OrphanType::PendingNonTip));
                            }
                        }
                    }

                    if replace_state {
                        // Replace current pending head with the
                        // pushed block
                        let val = self.pending_heads_parents.remove(&block_hash).unwrap();

                        // Move old state to new key
                        self.pending_heads_parents.insert(block.parent_hash().unwrap(), val);
                    }

                    Ok(())
                } else {
                    // If the parent exists and it is not the canonical
                    // tip this means that this block is represents a 
                    // potential fork in the chain so we add it to the
                    // orphan pool and the canonical tips cache.
                    match self.db.get(&parent_hash) {
                        Some(_) => {
                            let block_hash = block.block_hash().unwrap();

                            // Write to orphan pool
                            self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block, OrphanType::CanonicalTip));

                            // Write to canonical tips cache
                            self.canonical_tips_cache.insert(block_hash);

                            Ok(())
                        }
                        None => {
                            // The parent is an orphan
                            if let Some(orphan) = self.orphan_pool.get(&parent_hash) {
                                let block_hash = block.block_hash().unwrap();

                                match orphan.orphan_type() {
                                    OrphanType::PendingTip => {
                                        panic!("The parent cannot be a pending tip at this point!");
                                    }
                                    OrphanType::CanonicalTip => {
                                        panic!("The parent cannot be a canonical tip at this point!");
                                    }
                                    OrphanType::PendingTipHead => {
                                        panic!("The parent cannot be a pending tip and head at this point!");
                                    }
                                    OrphanType::PendingNonTip 
                                    | OrphanType::PendingHead => {
                                        // Write to orphan pool
                                        self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block, OrphanType::PendingTip));

                                        // Write to pending tips cache
                                        self.pending_tips_cache.insert(block_hash);
                                    }
                                    OrphanType::CanonicalNonTip => {
                                        // Write to orphan pool
                                        self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block, OrphanType::CanonicalTip));

                                        // Write to canonical tips cache
                                        self.canonical_tips_cache.insert(block_hash);
                                    }
                                };

                                Ok(())
                            } else {
                                // Create a new non-canonical chain
                                let parent_hash = block.parent_hash().unwrap();

                                // Write to orphan pool
                                self.orphan_pool.insert(block_hash.clone(), OrphanState::new(block, OrphanType::PendingTipHead));
                            
                                // Write as pending tip
                                self.pending_tips_cache.insert(block_hash.clone());

                                // And as pending head
                                let mut branches = Branches::new();
                                branches.insert(block_hash, &[], None);

                                self.pending_heads_parents.insert(parent_hash, branches);

                                Ok(())
                            }
                        }
                    }
                }
            }
        } else {
            Err(ChainErr::NoParentHash)
        }
    }

    fn height(&self) -> u64 {
        self.height
    }

    fn canonical_tip(&self) -> Arc<HardBlock> {
        self.canonical_tip.clone()
    }

    // fn iter_canonical_tips(&'a self) -> HardBlockIterator<'a> {
    //     HardBlockIterator(Box::new(
    //         self.canonical_tips_cache.iter().map(|t| self.query(t).unwrap()).map(AsRef::as_ref),
    //     ))
    // }

    // fn iter_pending_tips(&'a self) -> HardBlockIterator<'a> {
    //     HardBlockIterator(Box::new(
    //         self.pending_tips_cache.iter().map(|t| self.query(t).unwrap()).map(AsRef::as_ref),
    //     ))
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::easy_chain::chain::EasyChain;
    use rand::*;
    use quickcheck::*;

    quickcheck! {
        /// Stress test of chain append.
        /// 
        /// We have blocks of the following structure:
        /// ```
        /// GEN -> A -> B -> C -> D -> E -> F -> G
        ///        |
        ///         -> B' -> C' -> D' -> E'
        ///            |     |
        ///            |     -> D'''
        ///            |
        ///            -> C'' -> D'' -> E'' -> F''
        /// ```
        /// 
        /// The tip of the block must always be `G`, regardless
        /// of the order in which the blocks are received. And 
        /// the height of the chain must be that of `G` which is 7.
        fn append_stress_test() -> bool {
            let db = test_helpers::init_tempdb();
            let easy_chain = Arc::new(RwLock::new(EasyChain::new(db.clone())));
            let easy_ref = EasyChainRef::new(easy_chain);
            let mut hard_chain = HardChain::new(db, easy_ref);

            let mut A = HardBlock::new(Some(HardChain::genesis().block_hash().unwrap()), 1, EasyChain::genesis().block_hash().unwrap());
            A.calculate_merkle_root();
            A.compute_hash();
            let A = Arc::new(A);

            let mut B = HardBlock::new(Some(A.block_hash().unwrap()), 2, EasyChain::genesis().block_hash().unwrap());
            B.calculate_merkle_root();
            B.compute_hash();
            let B = Arc::new(B);

            let mut C = HardBlock::new(Some(B.block_hash().unwrap()), 3, EasyChain::genesis().block_hash().unwrap());
            C.calculate_merkle_root();
            C.compute_hash();
            let C = Arc::new(C);

            let mut D = HardBlock::new(Some(C.block_hash().unwrap()), 4, EasyChain::genesis().block_hash().unwrap());
            D.calculate_merkle_root();
            D.compute_hash();
            let D = Arc::new(D);

            let mut E = HardBlock::new(Some(D.block_hash().unwrap()), 5, EasyChain::genesis().block_hash().unwrap());
            E.calculate_merkle_root();
            E.compute_hash();
            let E = Arc::new(E);

            let mut F = HardBlock::new(Some(E.block_hash().unwrap()), 6, EasyChain::genesis().block_hash().unwrap());
            F.calculate_merkle_root();
            F.compute_hash();
            let F = Arc::new(F);

            let mut G = HardBlock::new(Some(F.block_hash().unwrap()), 7, EasyChain::genesis().block_hash().unwrap());
            G.calculate_merkle_root();
            G.compute_hash();
            let G = Arc::new(G);

            let mut B_prime = HardBlock::new(Some(A.block_hash().unwrap()), 2, EasyChain::genesis().block_hash().unwrap());
            B_prime.calculate_merkle_root();
            B_prime.compute_hash();
            let B_prime = Arc::new(B_prime);

            let mut C_prime = HardBlock::new(Some(B_prime.block_hash().unwrap()), 3, EasyChain::genesis().block_hash().unwrap());
            C_prime.calculate_merkle_root();
            C_prime.compute_hash();
            let C_prime = Arc::new(C_prime);

            let mut D_prime = HardBlock::new(Some(C_prime.block_hash().unwrap()), 4, EasyChain::genesis().block_hash().unwrap());
            D_prime.calculate_merkle_root();
            D_prime.compute_hash();
            let D_prime = Arc::new(D_prime);

            let mut E_prime = HardBlock::new(Some(D_prime.block_hash().unwrap()), 5, EasyChain::genesis().block_hash().unwrap());
            E_prime.calculate_merkle_root();
            E_prime.compute_hash();
            let E_prime = Arc::new(E_prime);

            let mut C_second = HardBlock::new(Some(B_prime.block_hash().unwrap()), 3, EasyChain::genesis().block_hash().unwrap());
            C_second.calculate_merkle_root();
            C_second.compute_hash();
            let C_second = Arc::new(C_second);

            let mut D_second = HardBlock::new(Some(C_second.block_hash().unwrap()), 4, EasyChain::genesis().block_hash().unwrap());
            D_second.calculate_merkle_root();
            D_second.compute_hash();
            let D_second = Arc::new(D_second);

            let mut E_second = HardBlock::new(Some(D_second.block_hash().unwrap()), 5, EasyChain::genesis().block_hash().unwrap());
            E_second.calculate_merkle_root();
            E_second.compute_hash();
            let E_second = Arc::new(E_second);

            let mut F_second = HardBlock::new(Some(E_second.block_hash().unwrap()), 6, EasyChain::genesis().block_hash().unwrap());
            F_second.calculate_merkle_root();
            F_second.compute_hash();
            let F_second = Arc::new(F_second);

            let mut D_tertiary = HardBlock::new(Some(C_prime.block_hash().unwrap()), 4, EasyChain::genesis().block_hash().unwrap());
            D_tertiary.calculate_merkle_root();
            D_tertiary.compute_hash();
            let D_tertiary = Arc::new(D_tertiary);

            let mut blocks = vec![
                A,
                B,
                C,
                D,
                E,
                F,
                G.clone(),
                B_prime,
                C_prime,
                D_prime,
                E_prime,
                C_second,
                D_second,
                E_second,
                F_second,
                D_tertiary
            ];

            // Shuffle blocks
            thread_rng().shuffle(&mut blocks);

            for b in blocks {
                hard_chain.append_block(b).unwrap();
            }

            assert_eq!(hard_chain.height(), 7);
            assert_eq!(hard_chain.canonical_tip(), G);

            true
        }
    }
}