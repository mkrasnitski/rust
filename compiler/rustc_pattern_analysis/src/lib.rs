//! Analysis of patterns, notably match exhaustiveness checking.

pub mod constructor;
#[cfg(feature = "rustc")]
pub mod errors;
#[cfg(feature = "rustc")]
pub(crate) mod lints;
pub mod pat;
#[cfg(feature = "rustc")]
pub mod rustc;
pub mod usefulness;

#[macro_use]
extern crate tracing;
#[cfg(feature = "rustc")]
#[macro_use]
extern crate rustc_middle;

#[cfg(feature = "rustc")]
rustc_fluent_macro::fluent_messages! { "../messages.ftl" }

use std::fmt;

use rustc_index::Idx;
#[cfg(feature = "rustc")]
use rustc_middle::ty::Ty;
#[cfg(feature = "rustc")]
use rustc_span::ErrorGuaranteed;

use crate::constructor::{Constructor, ConstructorSet, IntRange};
#[cfg(feature = "rustc")]
use crate::lints::{lint_nonexhaustive_missing_variants, PatternColumn};
use crate::pat::DeconstructedPat;
#[cfg(feature = "rustc")]
use crate::rustc::RustcMatchCheckCtxt;
#[cfg(feature = "rustc")]
use crate::usefulness::{compute_match_usefulness, ValidityConstraint};

pub trait Captures<'a> {}
impl<'a, T: ?Sized> Captures<'a> for T {}

/// Context that provides type information about constructors.
///
/// Most of the crate is parameterized on a type that implements this trait.
pub trait TypeCx: Sized + fmt::Debug {
    /// The type of a pattern.
    type Ty: Copy + Clone + fmt::Debug; // FIXME: remove Copy
    /// Errors that can abort analysis.
    type Error: fmt::Debug;
    /// The index of an enum variant.
    type VariantIdx: Clone + Idx;
    /// A string literal
    type StrLit: Clone + PartialEq + fmt::Debug;
    /// Extra data to store in a match arm.
    type ArmData: Copy + Clone + fmt::Debug;
    /// Extra data to store in a pattern.
    type PatData: Clone;

    fn is_exhaustive_patterns_feature_on(&self) -> bool;

    /// The number of fields for this constructor.
    fn ctor_arity(&self, ctor: &Constructor<Self>, ty: Self::Ty) -> usize;

    /// The types of the fields for this constructor. The result must have a length of
    /// `ctor_arity()`.
    fn ctor_sub_tys(&self, ctor: &Constructor<Self>, ty: Self::Ty) -> &[Self::Ty];

    /// The set of all the constructors for `ty`.
    ///
    /// This must follow the invariants of `ConstructorSet`
    fn ctors_for_ty(&self, ty: Self::Ty) -> Result<ConstructorSet<Self>, Self::Error>;

    /// Best-effort `Debug` implementation.
    fn debug_pat(f: &mut fmt::Formatter<'_>, pat: &DeconstructedPat<'_, Self>) -> fmt::Result;

    /// Raise a bug.
    fn bug(&self, fmt: fmt::Arguments<'_>) -> !;

    /// Lint that the range `pat` overlapped with all the ranges in `overlaps_with`, where the range
    /// they overlapped over is `overlaps_on`. We only detect singleton overlaps.
    /// The default implementation does nothing.
    fn lint_overlapping_range_endpoints(
        &self,
        _pat: &DeconstructedPat<'_, Self>,
        _overlaps_on: IntRange,
        _overlaps_with: &[&DeconstructedPat<'_, Self>],
    ) {
    }
}

/// Context that provides information global to a match.
#[derive(derivative::Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct MatchCtxt<'a, Cx: TypeCx> {
    /// The context for type information.
    pub tycx: &'a Cx,
}

/// The arm of a match expression.
#[derive(Debug)]
#[derive(derivative::Derivative)]
#[derivative(Clone(bound = ""), Copy(bound = ""))]
pub struct MatchArm<'p, Cx: TypeCx> {
    pub pat: &'p DeconstructedPat<'p, Cx>,
    pub has_guard: bool,
    pub arm_data: Cx::ArmData,
}

/// The entrypoint for this crate. Computes whether a match is exhaustive and which of its arms are
/// useful, and runs some lints.
#[cfg(feature = "rustc")]
pub fn analyze_match<'p, 'tcx>(
    tycx: &RustcMatchCheckCtxt<'p, 'tcx>,
    arms: &[rustc::MatchArm<'p, 'tcx>],
    scrut_ty: Ty<'tcx>,
) -> Result<rustc::UsefulnessReport<'p, 'tcx>, ErrorGuaranteed> {
    let scrut_ty = tycx.reveal_opaque_ty(scrut_ty);
    let scrut_validity = ValidityConstraint::from_bool(tycx.known_valid_scrutinee);
    let cx = MatchCtxt { tycx };

    let report = compute_match_usefulness(cx, arms, scrut_ty, scrut_validity)?;

    // Run the non_exhaustive_omitted_patterns lint. Only run on refutable patterns to avoid hitting
    // `if let`s. Only run if the match is exhaustive otherwise the error is redundant.
    if tycx.refutable && report.non_exhaustiveness_witnesses.is_empty() {
        let pat_column = PatternColumn::new(arms);
        lint_nonexhaustive_missing_variants(cx, arms, &pat_column, scrut_ty)?;
    }

    Ok(report)
}
