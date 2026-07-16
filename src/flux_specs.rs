// extern crate alloc;
use flux_rs::*;

// -----------------------------------------------------------------------
// f64 methods from core (min, max, clamp, abs, signum, is_finite)
// Paths match the def path prefix: core::f64::{impl#0}::method
// -----------------------------------------------------------------------
#[extern_spec(core::num)]
impl f64 {
    #[no_panic]
    fn min(self, other: f64) -> f64;

    #[no_panic]
    fn max(self, other: f64) -> f64;

    #[no_panic]
    fn clamp(self, min: f64, max: f64) -> f64;

    #[no_panic]
    fn abs(self) -> f64;

    #[no_panic]
    fn signum(self) -> f64;

    #[no_panic]
    fn is_finite(self) -> bool;
}

// -----------------------------------------------------------------------
// usize unchecked arithmetic.
//
// These are `unsafe fn`s whose real safety contract is "the operation must
// not overflow" (checked at runtime by the stdlib's internal
// `assert_unsafe_precondition!` guard, which panics via `panic_nounwind_fmt`
// if violated). The preconditions below are lifted directly from that guard
// (`!lhs.overflowing_sub(rhs).1` / `!lhs.overflowing_add(rhs).1`), matching
// the success condition already used for `checked_sub`/`checked_add` in
// flux-core. Given the precondition, the debug-mode guard can never fire, so
// `no_panic` is sound.
// TODO: port these into flux-core's `uint_spec!`/`int_spec!` macros
// (lib/flux-core/src/num/mod.rs) so all integer widths get them.
// -----------------------------------------------------------------------
#[extern_spec(core::num)]
impl usize {
    #[no_panic]
    #[spec(fn(num: usize, rhs: usize{rhs <= num}) -> usize[num - rhs])]
    unsafe fn unchecked_sub(self, rhs: usize) -> usize;

    #[no_panic]
    #[spec(fn(num: usize, rhs: usize{num + rhs <= usize::MAX}) -> usize[num + rhs])]
    unsafe fn unchecked_add(self, rhs: usize) -> usize;
}

// isize::unchecked_neg — same reasoning as above. Real guard is
// `!lhs.overflowing_neg().1`, i.e. `num != isize::MIN` (negating MIN overflows).
#[extern_spec(core::num)]
impl isize {
    #[no_panic]
    #[spec(fn(num: isize{num != isize::MIN}) -> isize[-num])]
    unsafe fn unchecked_neg(self) -> isize;
}

// -----------------------------------------------------------------------
// f64 methods from std (exp, floor, ceil, sqrt)
// Paths match the def path prefix: std::f64::{impl#0}::method
// -----------------------------------------------------------------------
#[extern_spec(std::f64)]
impl f64 {
    #[no_panic]
    fn exp(self) -> f64;

    #[no_panic]
    fn floor(self) -> f64;

    #[no_panic]
    fn ceil(self) -> f64;

    #[no_panic]
    fn sqrt(self) -> f64;
}

use std::{
    alloc::{Allocator, Global},
    ops::{Deref, DerefMut, Index, IndexMut},
    slice::SliceIndex,
};

// use flux_attrs::*;

//---------------------------------------------------------------------------------------
#[extern_spec]
#[refined_by(len: int)]
#[invariant(0 <= len)]
struct Vec<T, A: Allocator = Global>;

//---------------------------------------------------------------------------------------

#[extern_spec]
impl<T> Vec<T> {
    #[no_panic]
    #[flux::sig(fn() -> Vec<T>[0])]
    fn new() -> Vec<T>;

    #[no_panic]
    #[spec(fn(usize) -> Vec<T>[0])]
    fn with_capacity(capacity: usize) -> Vec<T>;
}

#[extern_spec]
impl<T, A: Allocator> Vec<T, A> {
    #[no_panic]
    #[spec(fn(self: &mut Vec<T, A>[@n], T) ensures self: Vec<T, A>[n+1])]
    fn push(v: &mut Vec<T, A>, value: T);

    #[no_panic]
    #[spec(fn(&Vec<T, A>[@n]) -> usize[n])]
    fn len(v: &Vec<T, A>) -> usize;

    #[no_panic]
    #[spec(fn(self: &mut Vec<T, A>[@n]) -> Option<T>[n > 0] ensures self: Vec<T, A>[if n > 0 { n-1 } else { 0 }])]
    fn pop(&mut self) -> Option<T>;

    #[no_panic]
    #[spec(fn(self: &Vec<T, A>[@n]) -> bool[n == 0])]
    fn is_empty(&self) -> bool;
}

#[extern_spec]
impl<T: Clone, A: Allocator + Clone> Clone for Vec<T, A> {
    #[no_panic]
    #[spec(fn(self: &Vec<T, A>[@n]) -> Vec<T, A>[n])]
    fn clone(&self) -> Vec<T, A>;
}

//---------------------------------------------------------------------------------------

#[extern_spec]
impl<T, I: SliceIndex<[T]>, A: Allocator> Index<I> for Vec<T, A> {
    #[no_panic]
    #[assume_parametric(T)]
    #[spec(fn(&Vec<T, A>[@len], {I[@idx] | <I as SliceIndex<[T]>>::in_bounds(idx, len)}) -> _)]
    fn index(z: &Vec<T, A>, index: I) -> &<I as SliceIndex<[T]>>::Output;
}

#[extern_spec]
impl<T, I: SliceIndex<[T]>, A: Allocator> IndexMut<I> for Vec<T, A> {
    #[no_panic]
    #[assume_parametric(T)]
    #[spec(fn(&mut Vec<T,A>[@len], {I[@idx] | <I as SliceIndex<[T]>>::in_bounds(idx, len)}) -> _)]
    fn index_mut(z: &mut Vec<T, A>, index: I) -> &mut <I as SliceIndex<[T]>>::Output;
}

//---------------------------------------------------------------------------------------
#[extern_spec]
impl<'a, T, A: Allocator> IntoIterator for &'a Vec<T, A> {
    #[no_panic]
    #[spec(fn (&Vec<T, A>[@n]) -> <&Vec<T, A> as IntoIterator>::IntoIter[0,n])]
    fn into_iter(v: &'a Vec<T, A>) -> <&'a Vec<T, A> as IntoIterator>::IntoIter;
}

#[extern_spec]
#[assoc(fn with_size(self: Self, n:int) -> bool { self.len == n })]
impl<T> FromIterator<T> for Vec<T> {}

// ---------------------------------------------------------------------------------------

#[extern_spec(std::vec)]
#[assoc(fn as_deref(v: Self, target: int) -> bool { v.len == target })]
impl<T, A: Allocator> Deref for Vec<T, A> {
    #[no_panic]
    #[sig(fn(self: &Self[@v]) -> &[T][v])]
    fn deref(&self) -> &[T];
}

#[extern_spec(std::vec)]
impl<T, A: Allocator> DerefMut for Vec<T, A> {
    #[no_panic]
    #[sig(fn(self: &mut Self[@v]) -> &mut [T][v])]
    fn deref_mut(&mut self) -> &mut [T];
}

// ---------------------------------------------------------------------------------------
// BTreeMap
// ---------------------------------------------------------------------------------------

use std::collections::{btree_map::Entry, BTreeMap};

// entry, insert — impl<K, V, A: Allocator + Clone> BTreeMap<K, V, A> (impl#20)
// (get omitted: its return lifetime cannot be expressed without an explicit early-bound
//  lifetime parameter, which conflicts with the late-bound &self lifetime in the real impl)
#[extern_spec]
impl<K: Ord, V, A: Allocator + Clone> BTreeMap<K, V, A> {
    #[no_panic]
    fn entry(&mut self, key: K) -> Entry<'_, K, V, A>;

    #[no_panic]
    fn insert(&mut self, key: K, value: V) -> Option<V>;
}

// or_default — impl<'a, K: Ord, V: Default, A: Allocator + Clone> Entry<'a, K, V, A> (entry impl#7)
#[extern_spec]
impl<'a, K: Ord, V: Default, A: Allocator + Clone> Entry<'a, K, V, A> {
    #[no_panic]
    fn or_default(self) -> &'a mut V;
}

// next — impl<'a, K: 'a, V: 'a> Iterator for Iter<'a, K, V> (impl#22, no allocator param)
#[extern_spec(std::collections::btree_map)]
impl<'a, K: 'a, V: 'a> Iterator for Iter<'a, K, V> {
    #[no_panic]
    fn next(&mut self) -> Option<(&'a K, &'a V)>;
}

// ---------------------------------------------------------------------------------------
// slice::Iter — all/any
//
// flux-core's iter.rs only specs `next`; it doesn't cover the closure-taking
// combinators, so any call to them under a `no_panic` obligation is flagged as
// possibly panicking. These take a closure, so they need `no_panic_if` rather
// than a plain `no_panic` (see flux's CLAUDE.md "Closures and panic").
// TODO: port these into flux-core/src/slice/iter.rs so they're covered generally.
// ---------------------------------------------------------------------------------------
#[extern_spec(core::slice)]
impl<'a, T> Iterator for Iter<'a, T> {
    #[flux_rs::no_panic_if(F::no_panic())]
    #[spec(fn(&mut Iter<T>[@it], F) -> bool)]
    fn all<F: FnMut(&'a T) -> bool>(&mut self, f: F) -> bool;

    #[flux_rs::no_panic_if(F::no_panic())]
    #[spec(fn(&mut Iter<T>[@it], F) -> bool)]
    fn any<F: FnMut(&'a T) -> bool>(&mut self, f: F) -> bool;

    #[flux_rs::no_panic_if(F::no_panic())]
    #[spec(fn(Iter<T>, B, F) -> B)]
    fn fold<B, F: FnMut(B, &'a T) -> B>(self, init: B, f: F) -> B;
}

// ---------------------------------------------------------------------------------------
// slice::Iter — next_back
//
// `next_back`'s real implementation is `#[inline]` all the way down through
// `NonNull::sub`'s call to `isize::unchecked_neg`. Giving `unchecked_neg` its
// own sound spec (see above) doesn't help here: MIR inlining flattens that
// whole call chain into `next_back`'s body before extern-spec substitution
// can intercept the (now-vanished) call to `unchecked_neg`, leaving only a
// call to the macro-generated `precondition_check` helper, which has no
// stable path and can't be extern-spec'd directly. So `next_back` itself
// needs a trusted contract, mirroring `next`'s existing spec in flux-core's
// iter.rs (idx unchanged, len decreases by 1, instead of idx+1/len unchanged).
// TODO: port into flux-core/src/slice/iter.rs alongside `next`.
// ---------------------------------------------------------------------------------------
#[extern_spec(core::slice)]
impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    #[no_panic]
    #[spec(fn(self: &mut Iter<T>[@curr_s]) -> Option<_>[curr_s.idx < curr_s.len]
           ensures self: Iter<T>{next_s: curr_s.idx == next_s.idx && next_s.len == curr_s.len - 1})]
    fn next_back(&mut self) -> Option<&'a T>;
}
