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

use std::collections::{
    btree_map::{Entry, Iter},
    BTreeMap,
};

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
