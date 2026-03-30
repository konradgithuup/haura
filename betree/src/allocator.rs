//! This module provides `SegmentAllocator` and `SegmentId` for bitmap
//! allocation of 1GiB segments.

use crate::{cow_bytes::CowBytes, storage_pool::DiskOffset, vdev::Block, Error};
use bitvec::prelude::*;
use byteorder::{BigEndian, ByteOrder};
use std::io::Write;

/// 256KiB, so that `vdev::BLOCK_SIZE * SEGMENT_SIZE == 1GiB`
pub const SEGMENT_SIZE: usize = 1 << SEGMENT_SIZE_LOG_2;
/// Number of bytes required to store a segments allocation bitmap
pub const SEGMENT_SIZE_BYTES: usize = SEGMENT_SIZE / 8;
const SEGMENT_SIZE_LOG_2: usize = 18;
const SEGMENT_SIZE_MASK: usize = SEGMENT_SIZE - 1;

use super::*;

/// Simple first-fit bitmap allocator that uses a list to manage free segments
pub struct SegmentAllocator {
    data: BitArr!(for SEGMENT_SIZE, in u8, Lsb0),
    free_segments: Vec<(u32, u32)>, // (offset, size) of free segments
}

impl SegmentAllocator {
    /// Constructs a new `SegmentAllocator` given the segment allocation bitmap.
    /// The `bitmap` must have a length of `SEGMENT_SIZE`.
    pub fn new(bitmap: [u8; SEGMENT_SIZE_BYTES]) -> Self {
        let data = BitArray::new(bitmap);
        let mut allocator = SegmentAllocator {
            data,
            free_segments: Vec::new(),
        };
        allocator.initialize_free_segments();
        allocator
    }

    /// Allocates a block of the given `size`.
    /// Returns `None` if the allocation request cannot be satisfied and the offset if if can.
    pub fn allocate(&mut self, size: u32) -> Option<u32> {
        if size == 0 {
            return Some(0);
        }

        for i in 0..self.free_segments.len() {
            let (offset, segment_size) = self.free_segments[i];

            if segment_size >= size {
                self.mark(offset, size, Action::Allocate);

                // update the free segment with the remaining size and new offset
                self.free_segments[i].0 = offset + size;
                self.free_segments[i].1 = segment_size - size;
                // NOTE: We do not handle the == case here. We could remove that entry from the
                // list but we then would need to copy some things because the allocate_at (and
                // deallocation) logic depends on a sorted list and also need have extra handling.
                // The empty slots get garbage collected on the next sync anyway.
                return Some(offset);
            }
        }
        None
    }

    /// Allocates a block of the given `size` at `offset`.
    /// Returns `false` if the allocation request cannot be satisfied.
    pub fn allocate_at(&mut self, size: u32, offset: u32) -> bool {
        if size == 0 {
            return true;
        }
        if offset + size > SEGMENT_SIZE as u32 {
            return false;
        }

        let start_idx = offset as usize;
        let end_idx = (offset + size) as usize;
        if self.data[start_idx..end_idx].any() {
            return false;
        }

        // Update free_segments to reflect the allocation
        for i in 0..self.free_segments.len() {
            let (seg_offset, seg_size) = self.free_segments[i];
            if seg_offset == offset && seg_size == size {
                // perfect fit, remove the segment
                self.free_segments.remove(i);
                self.mark(offset, size, Action::Allocate);
                return true;
            } else if seg_offset == offset && seg_size > size {
                // allocation at the beginning of the segment, adjust offset and size
                self.free_segments[i].0 += size;
                self.free_segments[i].1 -= size;
                self.mark(offset, size, Action::Allocate);
                return true;
            } else if offset > seg_offset && offset + size == seg_offset + seg_size {
                // allocation at the end of the segment, just adjust size
                self.free_segments[i].1 -= size;
                self.mark(offset, size, Action::Allocate);
                return true;
            } else if offset > seg_offset
                && offset < seg_offset + seg_size
                && offset + size < seg_offset + seg_size
            {
                // allocation in the middle of the segment, split segment
                let remaining_size = seg_size - (size + (offset - seg_offset));
                let new_offset = offset + size;
                self.free_segments[i].1 = offset - seg_offset; // adjust current segment size

                self.free_segments
                    .insert(i + 1, (new_offset, remaining_size)); // insert new segment after current
                self.mark(offset, size, Action::Allocate);
                return true;
            }
        }

        false // No suitable free segment found in free_segments list
    }

    fn mark(&mut self, offset: u32, size: u32, action: Action) {
        let start_idx = offset as usize;
        let end_idx = (offset + size) as usize;
        let range = &mut self.data[start_idx..end_idx];

        match action {
            // Is allocation, so range must be free
            Action::Allocate => debug_assert!(!range.any()),
            // Is deallocation, so range must be previously used
            Action::Deallocate => debug_assert!(range.all()),
        }

        range.fill(action.as_bool());
    }

    /// Initializes the `free_segments` vector by scanning the bitmap.
    fn initialize_free_segments(&mut self) {
        let mut offset: u32 = 0;
        while offset < SEGMENT_SIZE as u32 {
            if !self.data[offset as usize] {
                // If bit is 0, it's free
                let start_offset = offset;
                let mut current_size = 0;
                while offset < SEGMENT_SIZE as u32 && !self.data[offset as usize] {
                    current_size += 1;
                    offset += 1;
                }
                self.free_segments.push((start_offset, current_size));
            } else {
                offset += 1;
            }
        }
    }
}

// TODO better wording
/// Allocation action
#[derive(Clone, Copy)]
pub enum Action {
    /// Deallocate an allocated block.
    Deallocate,
    /// Allocate a deallocated block.
    Allocate,
}

impl Action {
    /// Returns 1 if allocation and 0 if deallocation.
    pub fn as_bool(self) -> bool {
        match self {
            Action::Deallocate => false,
            Action::Allocate => true,
        }
    }
}

/// Identifier for 1GiB segments of a `StoragePool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegmentId(pub u64);

impl SegmentId {
    /// Returns the corresponding segment of the given disk offset.
    pub fn get(offset: DiskOffset) -> Self {
        SegmentId(offset.as_u64() & !(SEGMENT_SIZE_MASK as u64))
    }

    /// Returns the block offset into the segment.
    pub fn get_block_offset(offset: DiskOffset) -> u32 {
        offset.as_u64() as u32 & SEGMENT_SIZE_MASK as u32
    }

    /// Returns the disk offset at the start of this segment.
    pub fn as_disk_offset(&self) -> DiskOffset {
        DiskOffset::from_u64(self.0)
    }

    /// Returns the disk offset of the block in this segment at the given
    /// offset.
    pub fn disk_offset(&self, segment_offset: u32) -> DiskOffset {
        DiskOffset::from_u64(self.0 + u64::from(segment_offset))
    }

    /// Returns the key of this segment for messages and queries.
    pub fn key(&self, key_prefix: &[u8]) -> CowBytes {
        // Shave off the two lower bytes because they are always null.
        let mut segment_key = [0; 8];
        BigEndian::write_u64(&mut segment_key[..], self.0);
        assert_eq!(&segment_key[6..], &[0, 0]);

        let mut key = CowBytes::new();
        key.push_slice(key_prefix);
        key.push_slice(&segment_key[..6]);
        key
    }

    /// Returns the ID of the disk that belongs to this segment.
    pub fn disk_id(&self) -> u16 {
        self.as_disk_offset().disk_id()
    }

    /// Returns the next segment ID.
    /// Wraps around at the end of the disk.
    pub fn next(&self, disk_size: Block<u64>) -> SegmentId {
        let disk_offset = self.as_disk_offset();
        if disk_offset.block_offset().as_u64() + SEGMENT_SIZE as u64 >= disk_size.as_u64() {
            SegmentId::get(DiskOffset::new(
                disk_offset.storage_class(),
                disk_offset.disk_id(),
                Block(0),
            ))
        } else {
            SegmentId(self.0 + SEGMENT_SIZE as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_id() {
        let offset = DiskOffset::new(1, 2, Block::from_bytes(4096));
        let segment = SegmentId::get(offset);

        assert_eq!(segment.as_disk_offset().storage_class(), 1);
        assert_eq!(segment.disk_id(), 2);
        assert_eq!(
            segment.as_disk_offset().block_offset(),
            Block::from_bytes(0)
        );
        assert_eq!(SegmentId::get_block_offset(offset), 1);
    }
}
