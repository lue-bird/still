#![allow(dead_code, non_shorthand_field_patterns)]
#![allow(clippy::needless_pass_by_value, clippy::wrong_self_convention)]
#![no_implicit_prelude]
extern crate std;
use std::clone::Clone;
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;
use std::marker::Copy;
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
/// making this operation more expensive than `to_owned` or `clone`
///
/// ```
/// let mut still_state: Some_still_type::StillToOwned = ...;
/// let mut allocator = ...;
/// ..some_event_loop.. {
///     let old_state_still: Some_still_type = OwnedToStill::to_still(still_state);
///     let updated_state_still: Some_still_type =
///         some_still_fn(&allocator, old_state_still);
///     still_state = StillToOwned::to_owned(updated_state_still);
///     allocator.reset();
///  }
/// ```
///
/// See also `OwnedToStill`
pub trait StillToOwned {
    type Owned;
    fn to_owned(self) -> Self::Owned;
}
/// _Provided for any still value, for users of the generated code._
///
/// Take a fully owned value (one whose type does not have a lifetime)
/// and convert it to a still value, for example
/// `&std::vec::Vec<{ x: Box<str>, y: isize }>` gets turned into `Vec<'_, { x: Str<'_>, y: isize }>`
/// Notice how all _inner_ values are also turned into still values,
/// making this operation more expensive that simply borrowing.
///
/// See also `StillToOwned` which includes an example of how to use it
pub trait OwnedToStill {
    type Still<'a>
    where
        Self: 'a;
    fn to_still<'a>(&'a self) -> Self::Still<'a>;
}
impl<T: ?std::marker::Sized> OwnedToStill for std::boxed::Box<T> {
    type Still<'a>
        = &'a T
    where
        T: 'a;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        self
    }
}
impl<T: Clone> StillToOwned for &T {
    type Owned = std::boxed::Box<T>;
    fn to_owned(self) -> Self::Owned {
        std::boxed::Box::new(self.clone())
    }
}

pub type Int = isize;
impl OwnedToStill for Int {
    type Still<'a> = Int;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        *self
    }
}
impl StillToOwned for Int {
    type Owned = Int;
    fn to_owned(self) -> Self::Owned {
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
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        *self
    }
}
impl StillToOwned for Dec {
    type Owned = Dec;
    fn to_owned(self) -> Self::Owned {
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

pub enum Opt<A> {
    Present(A),
    Absent,
}
impl<A: StillToOwned> StillToOwned for Opt<A> {
    type Owned = Opt<A::Owned>;
    fn to_owned(self) -> Self::Owned {
        match self {
            Opt::Absent => Opt::Absent,
            Opt::Present(value) => Opt::Present(A::to_owned(value)),
        }
    }
}
impl<A: OwnedToStill> OwnedToStill for Opt<A> {
    type Still<'a>
        = Opt<A::Still<'a>>
    where
        A: 'a;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        match self {
            Opt::Absent => Opt::Absent,
            Opt::Present(value) => Opt::Present(A::to_still(value)),
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
impl StillToOwned for Blank {
    type Owned = Blank;
    fn to_owned(self) -> Self::Owned {
        self
    }
}
impl OwnedToStill for Blank {
    type Still<'a> = Blank;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        *self
    }
}

pub type Chr = char;
impl OwnedToStill for Chr {
    type Still<'a> = Chr;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        *self
    }
}
impl StillToOwned for Chr {
    type Owned = Chr;
    fn to_owned(self) -> Self::Owned {
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
impl StillToOwned for &str {
    type Owned = std::boxed::Box<str>;
    fn to_owned(self) -> Self::Owned {
        std::convert::Into::<std::boxed::Box<str>>::into(self)
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

/// Do not call `_.to_vec()` on it. Prefer `Rc::unwrap_or_clone` or `StillToOwned::to_owned`
pub type Vec<A> = std::rc::Rc<std::vec::Vec<A>>;
impl<A: Clone + StillToOwned> StillToOwned for Vec<A> {
    type Owned = std::vec::Vec<A::Owned>;
    fn to_owned(self) -> Self::Owned {
        match std::rc::Rc::try_unwrap(self) {
            std::result::Result::Ok(owned) => std::iter::Iterator::collect(
                std::iter::Iterator::map(std::iter::IntoIterator::into_iter(owned), A::to_owned),
            ),
            std::result::Result::Err(rc) => std::iter::Iterator::collect(std::iter::Iterator::map(
                std::iter::Iterator::cloned(rc.iter()),
                A::to_owned,
            )),
        }
    }
}
impl<A: OwnedToStill> OwnedToStill for std::vec::Vec<A> {
    type Still<'a>
        = Vec<A::Still<'a>>
    where
        A: 'a;
    fn to_still<'a>(&'a self) -> Self::Still<'a> {
        std::rc::Rc::new(std::iter::Iterator::collect(std::iter::Iterator::map(
            self.iter(),
            A::to_still,
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
