#![allow(
    dead_code,
    non_shorthand_field_patterns,
    non_camel_case_types,
    unused_imports,
    non_upper_case_globals
)]
#![allow(clippy::needless_pass_by_value, clippy::wrong_self_convention)]
#![no_implicit_prelude]
extern crate std;
use std::clone::Clone;
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;
use std::marker::Copy;
use std::ops::Fn;
// core //

/// bring your own bump/... allocator. For example:
/// ```
/// impl Alloc for bumpalo::Bump {
///     fn alloc<A>(&self, value: A) -> &A {
///         self.alloc(value)
///     }
/// }
/// ```
pub trait Alloc {
    fn alloc<A>(&self, value: A) -> &A;
}
fn alloc_fn_as_dyn<'a, Inputs, Output>(
    allocator: &'a impl Alloc,
    function: impl Fn(Inputs) -> Output + 'a,
) -> &'a dyn Fn(Inputs) -> Output {
    allocator.alloc(function)
}

/// _Provided for any still value, for users of the generated code._
///
/// When you execute still functions
/// whose state needs to be persisted in a long-running
/// program, you will need a representation
/// that is entirely self-owned and whose parts
/// don't point into some temporary memory allocator.
/// For example:
/// `Vec<'_, { x: Str<'_>, y: isize }>`
/// will get turned into
/// `std::vec::Vec<{ x: Box<str>, y: isize }`
/// Notice how all _inner_ values are also turned into still values,
/// making this operation more expensive than `into_owned` or `clone`
///
/// ```
/// let mut still_state: Some_still_type::Owned = StillIntoOwned::into_owned(...);
/// let mut allocator = ...;
/// ..some_event_loop.. {
///     let old_state_still: Some_still_type = OwnedToStill::to_still(still_state);
///     let updated_state_still: Some_still_type =
///         some_still_fn(&allocator, old_state_still);
///     StillIntoOwned::into_owned_overwriting(updated_state_still, &mut still_state);
///     allocator.reset();
///  }
/// ```
///
/// See also `OwnedToStill`
pub trait StillIntoOwned: std::marker::Sized {
    type Owned: Clone;
    fn into_owned(self) -> Self::Owned;
    /// `still.into_owned_overwriting(&mut owned)` is functionally equivalent to `owned = still.into_owned()`
    /// but can be overridden to reuse the allocations of `owned`.
    fn into_owned_overwriting(self, allocation_to_reuse: &mut Self::Owned) {
        *allocation_to_reuse = Self::into_owned(self);
    }
}
/// _Provided for any still value, for users of the generated code._
///
/// Take a fully owned value (one whose type does not have a lifetime)
/// and convert it to a still value, for example
/// `&std::vec::Vec<{ x: Box<str>, y: isize }>` gets turned into `Vec<'_, { x: Str<'_>, y: isize }>`
/// Notice how all _inner_ values are also turned into still values,
/// making this operation more expensive that simply borrowing.
///
/// See also `StillIntoOwned` which includes an example of how to use it
pub trait OwnedToStill {
    type Still<'a>
    where
        Self: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a>;
}
impl<T: ?std::marker::Sized + OwnedToStill> OwnedToStill for std::boxed::Box<T> {
    type Still<'a>
        = &'a T::Still<'a>
    where
        T: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        allocator.alloc(T::to_still(self, allocator))
    }
}
impl<T: StillIntoOwned + Clone> StillIntoOwned for &T {
    type Owned = std::boxed::Box<T::Owned>;
    fn into_owned(self) -> Self::Owned {
        std::boxed::Box::new(T::into_owned(self.clone()))
    }
    // TODO once std::boxed::Box::map becomes stable, use that to optimize into_owned_overwriting
}

pub type Int = isize;
impl OwnedToStill for Int {
    type Still<'a> = Int;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
}
impl StillIntoOwned for Int {
    type Owned = Int;
    fn into_owned(self) -> Self::Owned {
        self
    }
}

fn int_negate(int: Int) -> Int {
    -int
}
fn int_absolute(a: Int) -> Int {
    Int::abs(a)
}
fn int_add(a: Int, b: Int) -> Int {
    a + b
}
fn int_mul(a: Int, b: Int) -> Int {
    a * b
}
fn int_div(to_divide: Int, to_divide_by: Int) -> Int {
    to_divide / to_divide_by
}
fn int_to_str(allocator: &impl Alloc, int: Int) -> Str<'_> {
    allocator.alloc(std::format!("{}", int))
}
fn str_to_int(str: Str) -> Opt<Int> {
    match str.parse::<Int>() {
        std::result::Result::Err(_) => Opt::Absent,
        std::result::Result::Ok(int) => Opt::Present(int),
    }
}

pub type Dec = f32;
impl OwnedToStill for Dec {
    type Still<'a> = Dec;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
}
impl StillIntoOwned for Dec {
    type Owned = Dec;
    fn into_owned(self) -> Self::Owned {
        self
    }
}

fn dec_negate(dec: Dec) -> Dec {
    -dec
}
fn dec_absolute(a: Dec) -> Dec {
    Dec::abs(a)
}
fn dec_add(a: Dec, b: Dec) -> Dec {
    a + b
}
fn dec_mul(a: Dec, b: Dec) -> Dec {
    a * b
}
fn dec_div(to_divide: Dec, to_divide_by: Dec) -> Dec {
    to_divide / to_divide_by
}
fn dec_to_str(allocator: &impl Alloc, dec: Dec) -> Str<'_> {
    allocator.alloc(std::format!("{}", dec))
}
fn str_to_dec(str: Str) -> Opt<Dec> {
    match str.parse::<Dec>() {
        std::result::Result::Err(_) => Opt::Absent,
        std::result::Result::Ok(dec) => Opt::Present(dec),
    }
}
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Opt<A> {
    Present(A),
    Absent,
}
impl<A: StillIntoOwned + Clone> StillIntoOwned for Opt<A> {
    type Owned = Opt<A::Owned>;
    fn into_owned(self) -> Self::Owned {
        match self {
            Opt::Absent => Opt::Absent,
            Opt::Present(value) => Opt::Present(A::into_owned(value)),
        }
    }
    fn into_owned_overwriting(self, allocation_to_reuse: &mut Self::Owned) {
        match (self, allocation_to_reuse) {
            (Opt::Present(value), Opt::Present(value_allocation_to_reuse)) => {
                A::into_owned_overwriting(value, value_allocation_to_reuse);
            }
            (self_, allocation_to_reuse) => {
                *allocation_to_reuse = Self::into_owned(self_);
            }
        }
    }
}
impl<A: OwnedToStill> OwnedToStill for Opt<A> {
    type Still<'a>
        = Opt<A::Still<'a>>
    where
        A: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        match self {
            Opt::Absent => Opt::Absent,
            Opt::Present(value) => Opt::Present(A::to_still(value, allocator)),
        }
    }
}
impl<A> Opt<A> {
    fn from_option(option: std::option::Option<A>) -> Self {
        match option {
            std::option::Option::None => Opt::Absent,
            std::option::Option::Some(value) => Opt::Present(value),
        }
    }
    fn to_option(self) -> std::option::Option<A> {
        match self {
            Opt::Absent => std::option::Option::None,
            Opt::Present(value) => std::option::Option::Some(value),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Debug)]
pub struct Blank {}
impl StillIntoOwned for Blank {
    type Owned = Blank;
    fn into_owned(self) -> Self::Owned {
        self
    }
}
impl OwnedToStill for Blank {
    type Still<'a> = Blank;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
}

pub type Chr = char;
impl OwnedToStill for Chr {
    type Still<'a> = Chr;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
}
impl StillIntoOwned for Chr {
    type Owned = Chr;
    fn into_owned(self) -> Self::Owned {
        self
    }
}

fn chr_byte_count(chr: Chr) -> Int {
    chr.len_utf8() as Int
}
fn chr_to_str(allocator: &impl Alloc, chr: Chr) -> Str<'_> {
    allocator.alloc(std::format!("{}", chr))
}

pub type Str<'a> = &'a str;
impl<'a> StillIntoOwned for Str<'a> {
    type Owned = std::boxed::Box<str>;
    fn into_owned(self) -> Self::Owned {
        std::convert::Into::<std::boxed::Box<str>>::into(self)
    }
}
impl OwnedToStill for std::boxed::Box<str> {
    type Still<'a> = Str<'a>;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
    }
}

fn str_byte_count(str: Str) -> Int {
    str.len() as Int
}
// TODO fn str_slice_from_byte_index_for_byte_count(start_index, slice_byte_count, str: Str) -> Str
fn str_chr_at_byte_index(byte_index: Int, str: Str) -> Opt<Chr> {
    Opt::from_option(
        str.get(str.ceil_char_boundary(byte_index as usize)..)
            .and_then(|chr_sub| std::iter::Iterator::next(&mut chr_sub.chars())),
    )
}
fn str_to_chrs(str: Str) -> Vec<Chr> {
    std::rc::Rc::new(std::iter::Iterator::collect(str.chars()))
}
fn chrs_to_str<'a>(allocator: &'a impl Alloc, chars: Vec<Chr>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(chars.iter()));
    allocator.alloc(string)
}

/// Do not call `_.to_vec()` on it. Prefer `Rc::unwrap_or_clone`
pub type Vec<A> = std::rc::Rc<std::vec::Vec<A>>;
impl<A: StillIntoOwned + Clone> StillIntoOwned for Vec<A> {
    type Owned = std::vec::Vec<A::Owned>;
    fn into_owned(self) -> Self::Owned {
        match std::rc::Rc::try_unwrap(self) {
            std::result::Result::Ok(owned) => std::iter::Iterator::collect(
                std::iter::Iterator::map(std::iter::IntoIterator::into_iter(owned), A::into_owned),
            ),
            std::result::Result::Err(rc) => std::iter::Iterator::collect(std::iter::Iterator::map(
                std::iter::Iterator::cloned(rc.iter()),
                A::into_owned,
            )),
        }
    }
    fn into_owned_overwriting(self, vec_allocation_to_reuse: &mut Self::Owned) {
        vec_allocation_to_reuse.clear();
        let vec_allocation_to_reuse_len: usize = vec_allocation_to_reuse.len();
        match std::rc::Rc::try_unwrap(self) {
            std::result::Result::Ok(mut owned) => {
                vec_allocation_to_reuse.truncate(owned.len());
                // could we iterate owned by_ref instead of drain?
                // I'm not sure if that would drop an element
                for (element_allocation_to_reuse, element) in std::iter::Iterator::zip(
                    vec_allocation_to_reuse.iter_mut(),
                    std::vec::Vec::drain(&mut owned, 0..vec_allocation_to_reuse_len),
                ) {
                    A::into_owned_overwriting(element, element_allocation_to_reuse);
                }
                std::iter::Extend::extend(
                    vec_allocation_to_reuse,
                    std::iter::Iterator::map(
                        std::iter::IntoIterator::into_iter(owned),
                        A::into_owned,
                    ),
                );
            }
            std::result::Result::Err(rc) => {
                vec_allocation_to_reuse.truncate(rc.len());
                for (element_allocation_to_reuse, element) in
                    std::iter::Iterator::zip(vec_allocation_to_reuse.iter_mut(), rc.iter())
                {
                    A::into_owned_overwriting(element.clone(), element_allocation_to_reuse);
                }
                std::iter::Extend::extend(
                    vec_allocation_to_reuse,
                    std::iter::Iterator::map(
                        std::iter::Iterator::skip(rc.iter(), vec_allocation_to_reuse.len()),
                        |element| A::into_owned(element.clone()),
                    ),
                );
            }
        }
    }
}
impl<A: OwnedToStill> OwnedToStill for std::vec::Vec<A> {
    type Still<'a>
        = Vec<A::Still<'a>>
    where
        A: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        std::rc::Rc::new(std::iter::Iterator::collect(std::iter::Iterator::map(
            self.iter(),
            |element_owned_ref| A::to_still(element_owned_ref, allocator),
        )))
    }
}
fn vec_literal<const N: usize, A>(elements: [A; N]) -> Vec<A> {
    std::rc::Rc::new(std::convert::Into::<std::vec::Vec<A>>::into(elements))
}
fn vec_repeat<A: Clone>(length: Int, element: A) -> Vec<A> {
    std::rc::Rc::new(std::iter::Iterator::collect(std::iter::repeat_n(
        element,
        length as usize,
    )))
}
fn vec_length<A>(vec: Vec<A>) -> Int {
    vec.len() as Int
}
fn vec_element<A: Clone>(index: Int, vec: Vec<A>) -> Opt<A> {
    match vec.get(index as usize) {
        std::option::Option::None => Opt::Absent,
        std::option::Option::Some(element) => Opt::Present(element.clone()),
    }
}
fn vec_take<A: Clone>(taken_length: Int, vec: Vec<A>) -> Vec<A> {
    match std::rc::Rc::try_unwrap(vec) {
        std::result::Result::Ok(mut owned_vec) => {
            owned_vec.truncate(taken_length as usize);
            std::rc::Rc::new(owned_vec)
        }
        std::result::Result::Err(vec_rc) => std::rc::Rc::new(
            vec_rc
                .get(..(taken_length as usize))
                .map(std::convert::Into::<std::vec::Vec<A>>::into)
                .unwrap_or_else(|| std::vec![]),
        ),
    }
}
fn vec_attach<A: Clone>(left: Vec<A>, right: Vec<A>) -> Vec<A> {
    let mut combined: std::vec::Vec<A> = std::rc::Rc::unwrap_or_clone(left);
    match std::rc::Rc::try_unwrap(right) {
        std::result::Result::Err(rc) => {
            combined.extend_from_slice(&rc);
        }
        std::result::Result::Ok(owned) => {
            std::iter::Extend::extend(&mut combined, owned);
        }
    }
    std::rc::Rc::new(combined)
}
fn vec_flatten<A: Clone>(vec_vec: Vec<Vec<A>>) -> Vec<A> {
    std::rc::Rc::new(match std::rc::Rc::try_unwrap(vec_vec) {
        std::result::Result::Err(vec_vec) => {
            std::iter::Iterator::collect(std::iter::Iterator::cloned(
                std::iter::Iterator::flat_map(vec_vec.iter(), |inner| inner.iter()),
            ))
        }
        std::result::Result::Ok(vec_vec) => {
            let mut flattened: std::vec::Vec<A> = std::vec::Vec::new();
            for inner in vec_vec {
                match std::rc::Rc::try_unwrap(inner) {
                    std::result::Result::Err(inner) => {
                        flattened.extend_from_slice(&inner);
                    }
                    std::result::Result::Ok(inner) => {
                        std::iter::Extend::extend(&mut flattened, inner);
                    }
                }
            }
            flattened
        }
    })
}
fn strs_flatten<'a>(allocator: &'a impl Alloc, vec_of_str: Vec<Str>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(vec_of_str.iter()));
    allocator.alloc(string)
}
