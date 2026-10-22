//! The selectors median's wrapper registers for itself. A mirror of
//! `median::class::RESERVED_SELECTORS` (a test in `median` checks that the two lists agree):
//! `#[sel]` rejects them with a spanned error, as `Class::add_sel` does at run time.
pub(crate) const RESERVED_SELECTORS: &[&str] = &[
    "notify", "assist", "inletinfo", "dsp64", "ft1", "ft2", "ft3", "ft4", "ft5", "ft6", "ft7",
    "ft8", "ft9", "in1", "in2", "in3", "in4", "in5", "in6", "in7", "in8", "in9",
];
