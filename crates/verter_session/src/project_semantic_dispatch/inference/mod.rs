//! The inference owner: the candidates a collecting session fixes from,
//! and the type each inference variable fixes to (`getInferredType`) — for
//! a conditional's `infer` declarations and a reverse mapped type
//! (`getTypeFromInference`), and for a signature's type parameters.

mod fixation;

pub(crate) use fixation::{winning_candidates, WinningCandidates};
