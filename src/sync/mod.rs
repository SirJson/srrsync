//! This module contains the transfer protocol handlers.
//!
//! The general architecture is as follows:
//!
//! ```plain
//! +--------+   new index   +------+
//! |        | +-----------> |      |
//! | Source |               | Sink |
//! |        | request block |      |
//! |        | <-----------+ |      |
//! |        |               |      |
//! |        |  send block   |      |
//! |        | +-----------> |      |
//! +--------+               +------+
//! ```
//!
//! First the old index is computed and loaded in full.
//!
//! Then, the new index is fed in either all at once or in a streaming fashion.
//!
//! The sink will request blocks that are missing from the destination,
//! which are fed in as they are received.

pub mod fs;

use std::path::{Path, PathBuf};

use crate::{Error, HashDigest};
use crate::index::Index;

/// The sink, representing where the files are being sent.
///
/// This is relative to a single process, e.g. the sending side has a sink
/// encapsulating some network protocol, and the receiving side has a sink that
/// actually updates files.
pub trait Sink {
    /// Start on a new file
    fn new_file(&mut self, path: &Path, modified: chrono::DateTime<chrono::Utc>) -> Result<(), Error>;

    /// Feed entry from the new index
    fn new_block(&mut self, hash: &HashDigest, size: usize) -> Result<(), Error>;

    /// Feed a block that was requested
    fn feed_block(&mut self, hash: &HashDigest, block: &[u8]) -> Result<(), Error>;

    /// Ask which blocks to get next
    fn next_requested_block(&mut self) -> Result<Option<HashDigest>, Error>;

    /// Are we waiting on blocks?
    fn is_missing_blocks(&self) -> Result<bool, Error>;
}

/// Events that are received from the index data.
pub enum IndexEvent {
    /// Start a new file (e.g. next `NewBlock` are blocks of that file)
    NewFile(PathBuf, chrono::DateTime<chrono::Utc>),

    /// Add a new block to the current file
    NewBlock(HashDigest, usize),

    /// End of the whole transfer
    End,
}

/// The source, representing where the files are coming from.
///
/// This is relative to a single process, e.g. the sending side has a source
/// that reads from files, and the receiving side has a source that reads from
/// the network.
pub trait Source {
    /// Get the next event from the index data
    fn next_from_index(&mut self) -> Result<Option<IndexEvent>, Error>;

    /// Asynchronously request a block from this source
    fn request_block(&mut self, hash: &HashDigest) -> Result<(), Error>;

    /// Get a block that was previously requested
    fn get_next_block(&mut self) -> Result<Option<(HashDigest, Vec<u8>)>, Error>;
}

pub trait SinkExt {
    /// Feed a whole new index
    fn new_index(&mut self, new_index: &Index) -> Result<(), Error>;
}

impl<S: Sink> SinkExt for S {
    /// Feed a whole new index
    fn new_index(&mut self, new_index: &Index) -> Result<(), Error> {
        // TODO: Go over index and feed it to new_file()/new_block()
        // Maybe can be more efficient? Don't know
        unimplemented!()
    }
}

pub fn do_stream<S: Sink, R: Source>(mut recv: S, mut  send: R) -> Result<(), Error> {
    let mut instructions = true;
    while instructions || recv.is_missing_blocks()? {
        // Things are done in order so that bandwidth is used in a smart way
        // For example, if you block on sending block data, you will have
        // received more block requests in the next loop, and you'll only
        // transmit (sender side) or process (receiver side) index instructions
        // when there's nothing better to do
        if let Some(hash) = recv.next_requested_block()? {
            // Block requests
            send.request_block(&hash)?; // can block on HTTP receiver side
        } else if let Some((hash, block)) =
            send.get_next_block()? // blocks on receiver side
        {
            // Block data
            recv.feed_block(&hash, &block)?; // blocks on sender side
        } else if let Some(event) = send.next_from_index()? {
            // Index instructions
            match event {
                IndexEvent::NewFile(path, modified) => {
                    recv.new_file(&path, modified)?
                }
                IndexEvent::NewBlock(hash, size) => {
                    recv.new_block(&hash, size)?
                }
                IndexEvent::End => {
                    instructions = false;
                }
            }
        }
    }
    Ok(())
}