//! Bounded observation of a repository's HEAD and working-tree state.

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the repository observation collector is the first production caller"
    )
)]
mod parse;
