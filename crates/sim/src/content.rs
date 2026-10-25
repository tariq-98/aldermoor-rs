//! Combat content: abilities, auras, weapons, specialties, monsters and combo rules. The tables
//! live in the `content` crate as RON files; this re-exports them under the sim's own name.

pub use content::combat::*;

pub mod progression;

/// The skills a downed player can use: fight on, push enemies away, bandage up.
pub fn downed_skills() -> [&'static str; 3] {
    DOWNED
}
