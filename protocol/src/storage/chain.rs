//! In-memory chain management with validation. Placeholder.

use super::block::Block;

/// Ordered chain of validated blocks.
#[derive(Debug, Clone, Default)]
pub struct Chain {
    blocks: Vec<Block>,
}

impl Chain {
    /// Appends a validated block to the chain tip.
    pub fn append(&mut self, block: Block) {
        self.blocks.push(block);
    }

    /// Returns the latest block, if any.
    pub fn tip(&self) -> Option<&Block> {
        self.blocks.last()
    }

    /// Returns the chain height (number of blocks).
    pub fn height(&self) -> u64 {
        self.blocks.len() as u64
    }
}
