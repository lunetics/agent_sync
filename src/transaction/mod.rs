//! What makes a mutating run restorable: the backup snapshot, the witness and
//! manifest written after it, and the signal traps armed before the first write.

pub mod backup;
pub mod interrupt;
pub mod manifest;
pub mod witness;
