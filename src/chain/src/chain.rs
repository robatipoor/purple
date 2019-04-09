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
use crate::block_iterator::BlockIterator;
use crypto::Hash;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum ChainErr {
    /// The parent of the given block is invalid
    InvalidParent,

    /// The given event does not have a parent hash
    NoParentHash,
}

/// Generic chain interface
pub trait Chain<B> where B: Block {
    /// Returns the current height of the canonical chain.
    fn height(&self) -> usize;

    /// Returns an atomic reference to the topmost block in the canonical chain. 
    fn canonical_top(&self) -> Arc<B>; 

    /// Returns an atomic reference to the genesis block in the chain.
    fn genesis(&self) -> Arc<B>;

    /// Attempts to append a new block to the chain.
    fn append_block(&mut self, block: Arc<B>) -> Result<(), ChainErr>; 

    /// Queries for a block by its hash.
    fn query(&self, hash: &Hash) -> Option<Arc<B>>;

    /// Queries for a block by its height. This function can only
    /// return blocks from the canonical chain.
    fn query_by_height(&self, height: usize) -> Option<Arc<B>>;

    /// Returns the block height of the block with the given hash, if any.
    fn block_height(&self, hash: &Hash) -> Option<usize>;

    /// Returns an iterator over all top blocks of non-canonical chains that 
    /// descend from the canonical chain. The top block of the canonical chain
    /// will **NOT** be included.
    /// 
    /// Note that this does not include blocks that are disconnected in any way
    /// from the canonical chain i.e. blocks written that we haven't received 
    /// the parent of.
    fn iter_canonical_tops(&self) -> BlockIterator<'_>;

    /// Returns an iterator over all of top blocks of chains that are 
    /// completely disconnected from the canonical chain.
    fn iter_pending_tops(&self) -> BlockIterator<'_>;
}