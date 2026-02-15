#![allow(
    dead_code,
    non_shorthand_field_patterns,
    non_camel_case_types,
    clippy::needless_pass_by_value,
    clippy::wrong_self_convention
)]
#![no_implicit_prelude]
extern crate std;
use std::clone::Clone;
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;
use std::marker::Copy;
use std::ops::Fn;
// core //

/// bring your own bump/... allocator. For example for [bumpalo](https://docs.rs/bumpalo/latest/bumpalo/index.html):
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
/// `Vec<'_, { x: Str<'_>, y: Int }>`
/// will get turned into
/// `std::vec::Vec<{ x: std::rc::Rc<String>, y: isize }`
/// Notice how all _inner_ values are also converted.
///
/// ```
/// let mut allocator = ...;
/// let mut still_state: <Some_still_type<'static> as StillIntoOwned>::Owned =
///     StillIntoOwned::into_owned(some_still_fn(&allocator));
/// ..some_event_loop.. {
///     let old_state_still: Some_still_type = OwnedToStill::into_still(&allocator, still_state);
///     let updated_state_still: Some_still_type =
///         some_still_fn(&allocator, old_state_still);
///     still_state = StillIntoOwned::into_owned(updated_state_still);
///     allocator.reset();
///  }
/// ```
/// See also `OwnedToStill`
pub trait StillIntoOwned: std::marker::Sized {
    type Owned: Clone;
    fn into_owned(self) -> Self::Owned;
    /// `still.into_owned_overwriting(&mut owned)` is functionally equivalent to `owned = still.into_owned()`
    /// but can be overridden to reuse the allocations of `owned`.
    /// Note that currently, since `to_still` takes a reference with a lifetime of the returned still,
    /// it often can't actually be used to then mutate the original state,
    /// so it's use is really limited, and an `into_sill` into `into_owned` is usually
    /// the way to go
    fn into_owned_overwriting(self, allocation_to_reuse: &mut Self::Owned) {
        *allocation_to_reuse = Self::into_owned(self);
    }
}
/// _Provided for any still value, for users of the generated code._
///
/// Take a fully owned value (one whose type does not have a lifetime)
/// and convert it to a still value, for example
/// `std::vec::Vec<{ x: Box<str>, y: isize }>` gets turned into `Vec<'_, { x: Str<'_>, y: Int }>`
/// Notice how all _inner_ values are also turned into still values,
/// making this operation way more expensive that simply borrowing.
///
/// See also `StillIntoOwned` which includes an example of how to use it
pub trait OwnedToStill {
    type Still<'a>
    where
        Self: 'a;
    fn into_still<'a>(self, allocator: &'a impl Alloc) -> Self::Still<'a>;
    /// like into_still but when you only have a reference available.
    /// Prefer `into_still` if possible to reuse allocations
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a>;
}
impl<T: OwnedToStill> OwnedToStill for std::boxed::Box<T> {
    type Still<'a>
        = &'a T::Still<'a>
    where
        T: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        allocator.alloc(T::to_still(self, allocator))
    }
    fn into_still<'a>(self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        allocator.alloc(T::into_still(*self, allocator))
    }
}
impl<T: StillIntoOwned + Clone> StillIntoOwned for &T {
    type Owned = std::boxed::Box<T::Owned>;
    fn into_owned(self) -> Self::Owned {
        std::boxed::Box::new(T::into_owned(self.clone()))
    }
    // once std::boxed::Box::map becomes stable, use that to optimize into_owned_overwriting
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
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
    }
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Debug)]
pub enum Order {
    Less = -1,
    Equal = 0,
    Greater = 1,
}
impl OwnedToStill for Order {
    type Still<'a> = Order;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
    }
}
impl StillIntoOwned for Order {
    type Owned = Order;
    fn into_owned(self) -> Self::Owned {
        self
    }
}
impl Order {
    pub fn to_ordering(self) -> std::cmp::Ordering {
        match self {
            Order::Less => std::cmp::Ordering::Less,
            Order::Equal => std::cmp::Ordering::Equal,
            Order::Greater => std::cmp::Ordering::Greater,
        }
    }
    pub fn from_ordering(order: std::cmp::Ordering) -> Order {
        match order {
            std::cmp::Ordering::Less => Order::Less,
            std::cmp::Ordering::Equal => Order::Equal,
            std::cmp::Ordering::Greater => Order::Greater,
        }
    }
}

pub type Unt = usize;
impl OwnedToStill for Unt {
    type Still<'a> = Unt;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
    }
}
impl StillIntoOwned for Unt {
    type Owned = Unt;
    fn into_owned(self) -> Self::Owned {
        self
    }
}

fn unt_add(a: Unt, b: Unt) -> Unt {
    a + b
}
fn unt_mul(a: Unt, b: Unt) -> Unt {
    a * b
}
fn unt_div(to_divide: Unt, to_divide_by: Unt) -> Unt {
    Unt::checked_div(to_divide, to_divide_by).unwrap_or(0)
}
fn unt_order(left: Unt, right: Unt) -> Order {
    Order::from_ordering(left.cmp(&right))
}
fn unt_to_int(unt: Unt) -> Int {
    unt as Int
}
#[expect(clippy::cast_precision_loss)]
fn unt_to_dec(unt: Unt) -> Dec {
    unt as f32
}
fn unt_to_str<'a>(unt: Unt) -> Str<'a> {
    Str::from_string(std::format!("{}", unt))
}
fn str_to_unt(str: Str) -> Opt<Unt> {
    match str.as_str().parse::<Unt>() {
        std::result::Result::Err(_) => Opt::Absent,
        std::result::Result::Ok(unt) => Opt::Present(unt),
    }
}

pub type Int = isize;
impl OwnedToStill for Int {
    type Still<'a> = Int;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
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
fn int_absolute(a: Int) -> Unt {
    Int::abs(a) as Unt
}
fn int_add(a: Int, b: Int) -> Int {
    a + b
}
fn int_mul(a: Int, b: Int) -> Int {
    a * b
}
fn int_div(to_divide: Int, to_divide_by: Int) -> Int {
    Int::checked_div(to_divide, to_divide_by).unwrap_or(0)
}
fn int_order(left: Int, right: Int) -> Order {
    Order::from_ordering(left.cmp(&right))
}
fn int_to_unt(int: Int) -> Opt<Unt> {
    Opt::from_option(std::convert::TryInto::<Unt>::try_into(int).ok())
}
#[expect(clippy::cast_precision_loss)]
fn int_to_dec(int: Int) -> Dec {
    int as f32
}
fn int_to_str<'a>(int: Int) -> Str<'a> {
    Str::from_string(std::format!("{}", int))
}
fn str_to_int(str: Str) -> Opt<Int> {
    match str.as_str().parse::<Int>() {
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
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
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
    if to_divide_by == 0.0 {
        0.0
    } else {
        to_divide / to_divide_by
    }
}
fn dec_to_power_of(dec: Dec, exponent: Dec) -> Dec {
    Dec::powf(dec, exponent)
}
fn dec_truncate(dec: Dec) -> Int {
    Dec::trunc(dec) as Int
}
fn dec_floor(dec: Dec) -> Int {
    Dec::floor(dec) as Int
}
fn dec_ceiling(dec: Dec) -> Int {
    Dec::ceil(dec) as Int
}
fn dec_round(dec: Dec) -> Int {
    Dec::round(dec) as Int
}
fn dec_order(left: Dec, right: Dec) -> Order {
    match left.partial_cmp(&right) {
        std::option::Option::Some(ordering) => Order::from_ordering(ordering),
        std::option::Option::None => Order::Equal,
    }
}
fn dec_to_str<'a>(dec: Dec) -> Str<'a> {
    Str::from_string(std::format!("{}", dec))
}
fn str_to_dec(str: Str) -> Opt<Dec> {
    match str.as_str().parse::<Dec>() {
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
    fn into_still<'a>(self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        match self {
            Opt::Absent => Opt::Absent,
            Opt::Present(value) => Opt::Present(A::into_still(value, allocator)),
        }
    }
}
impl<A> Opt<A> {
    pub fn from_option(option: std::option::Option<A>) -> Self {
        match option {
            std::option::Option::None => Opt::Absent,
            std::option::Option::Some(value) => Opt::Present(value),
        }
    }
    pub fn into_option(self) -> std::option::Option<A> {
        match self {
            Opt::Absent => std::option::Option::None,
            Opt::Present(value) => std::option::Option::Some(value),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Continue_or_exit<C, E> {
    Continue(C),
    Exit(E),
}
impl<C: StillIntoOwned + Clone, E: StillIntoOwned + Clone> StillIntoOwned
    for Continue_or_exit<C, E>
{
    type Owned = Continue_or_exit<C::Owned, E::Owned>;
    fn into_owned(self) -> Self::Owned {
        match self {
            Continue_or_exit::Continue(continue_) => {
                Continue_or_exit::Continue(C::into_owned(continue_))
            }
            Continue_or_exit::Exit(exit) => Continue_or_exit::Exit(E::into_owned(exit)),
        }
    }
    fn into_owned_overwriting(self, allocation_to_reuse: &mut Self::Owned) {
        match (self, allocation_to_reuse) {
            (
                Continue_or_exit::Continue(continue_),
                Continue_or_exit::Continue(continue_allocation_to_reuse),
            ) => {
                C::into_owned_overwriting(continue_, continue_allocation_to_reuse);
            }
            (Continue_or_exit::Exit(exit), Continue_or_exit::Exit(exit_allocation_to_reuse)) => {
                E::into_owned_overwriting(exit, exit_allocation_to_reuse);
            }
            (self_, allocation_to_reuse) => {
                *allocation_to_reuse = Self::into_owned(self_);
            }
        }
    }
}
impl<C: OwnedToStill, E: OwnedToStill> OwnedToStill for Continue_or_exit<C, E> {
    type Still<'a>
        = Continue_or_exit<C::Still<'a>, E::Still<'a>>
    where
        C: 'a,
        E: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        match self {
            Continue_or_exit::Continue(continue_) => {
                Continue_or_exit::Continue(C::to_still(continue_, allocator))
            }
            Continue_or_exit::Exit(exit) => Continue_or_exit::Exit(E::to_still(exit, allocator)),
        }
    }
    fn into_still<'a>(self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        match self {
            Continue_or_exit::Continue(continue_) => {
                Continue_or_exit::Continue(C::into_still(continue_, allocator))
            }
            Continue_or_exit::Exit(exit) => Continue_or_exit::Exit(E::into_still(exit, allocator)),
        }
    }
}
impl<C, E> Continue_or_exit<C, E> {
    fn to_control_flow(self) -> std::ops::ControlFlow<E, C> {
        match self {
            Continue_or_exit::Continue(continue_) => std::ops::ControlFlow::Continue(continue_),
            Continue_or_exit::Exit(exit) => std::ops::ControlFlow::Break(exit),
        }
    }
    fn from_control_flow(control_flow: std::ops::ControlFlow<E, C>) -> Self {
        match control_flow {
            std::ops::ControlFlow::Continue(continue_) => Continue_or_exit::Continue(continue_),
            std::ops::ControlFlow::Break(exit) => Continue_or_exit::Exit(exit),
        }
    }
}

pub type Chr = char;
impl OwnedToStill for Chr {
    type Still<'a> = Chr;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        *self
    }
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        self
    }
}
impl StillIntoOwned for Chr {
    type Owned = Chr;
    fn into_owned(self) -> Self::Owned {
        self
    }
}

fn chr_byte_count(chr: Chr) -> Unt {
    chr.len_utf8()
}
fn chr_order(left: Chr, right: Chr) -> Order {
    Order::from_ordering(left.cmp(&right))
}
fn code_point_to_chr(code_point: Unt) -> Opt<Chr> {
    Opt::from_option(
        std::convert::TryFrom::try_from(code_point)
            .ok()
            .and_then(char::from_u32),
    )
}
fn chr_to_code_point(chr: Chr) -> Unt {
    chr as Unt
}
fn chr_to_str<'a>(chr: Chr) -> Str<'a> {
    Str::from_string(std::format!("{}", chr))
}
/// prefer Str::into_string over Str::to_string
#[derive(Clone)]
pub enum Str<'a> {
    Rc(std::rc::Rc<std::string::String>),
    Slice(&'a str),
}
impl std::convert::AsRef<str> for Str<'_> {
    fn as_ref(&self) -> &'_ str {
        match self {
            Str::Rc(rc) => rc,
            Str::Slice(slice) => slice,
        }
    }
}
impl std::fmt::Debug for Str<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), formatter)
    }
}
impl std::fmt::Display for Str<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(self.as_str(), formatter)
    }
}
impl Str<'_> {
    pub fn as_str(&self) -> &'_ str {
        match self {
            Str::Rc(rc) => rc,
            Str::Slice(slice) => slice,
        }
    }
    pub fn into_string(self) -> std::string::String {
        match self {
            Str::Rc(rc) => std::rc::Rc::unwrap_or_clone(rc),
            Str::Slice(slice) => std::string::ToString::to_string(slice),
        }
    }
    pub fn from_string(string: std::string::String) -> Self {
        Str::Rc(std::rc::Rc::new(string))
    }
}
impl Eq for Str<'_> {}
impl PartialEq for Str<'_> {
    fn eq(&self, other: &Str<'_>) -> bool {
        self.as_str() == other.as_str()
    }
}
impl PartialEq<str> for Str<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl Ord for Str<'_> {
    fn cmp(&self, other: &Str<'_>) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl PartialOrd for Str<'_> {
    fn partial_cmp(&self, other: &Str<'_>) -> std::option::Option<std::cmp::Ordering> {
        std::option::Option::Some(self.cmp(other))
    }
}
impl PartialOrd<str> for Str<'_> {
    fn partial_cmp(&self, other: &str) -> std::option::Option<std::cmp::Ordering> {
        std::option::Option::Some(self.as_str().cmp(other))
    }
}
impl<'a> StillIntoOwned for Str<'a> {
    type Owned = std::rc::Rc<std::string::String>;
    fn into_owned(self) -> Self::Owned {
        match self {
            Str::Rc(rc) => rc,
            Str::Slice(slice) => std::rc::Rc::new(std::string::ToString::to_string(slice)),
        }
    }
}
impl OwnedToStill for std::rc::Rc<std::string::String> {
    type Still<'a> = Str<'a>;
    fn to_still<'a>(&'a self, _: &'a impl Alloc) -> Self::Still<'a> {
        Str::Slice(self)
    }
    fn into_still<'a>(self, _: &'a impl Alloc) -> Self::Still<'a> {
        Str::Rc(self)
    }
}

fn str_byte_count(str: Str) -> Unt {
    str.as_str().len()
}
fn str_chr_at_byte_index(str: Str, byte_index: Unt) -> Opt<Chr> {
    Opt::from_option(
        str.as_str()
            .get(str.as_str().ceil_char_boundary(byte_index)..)
            .and_then(|chr_sub| std::iter::Iterator::next(&mut chr_sub.chars())),
    )
}
fn str_slice_from_byte_index_with_byte_length<'a>(
    allocator: &'a impl Alloc,
    str: Str<'a>,
    start_index: Unt,
    slice_byte_length: Unt,
) -> Str<'a> {
    let slice: &str = match str {
        Str::Slice(slice) => slice,
        Str::Rc(rc) => allocator.alloc(rc),
    };
    Str::Slice(
        slice
            .get(
                slice.floor_char_boundary(start_index)
                    ..slice.ceil_char_boundary(start_index + slice_byte_length),
            )
            .unwrap_or(""),
    )
}
fn str_to_chrs(str: Str) -> Vec<Chr> {
    Vec::from_vec(std::iter::Iterator::collect(str.as_str().chars()))
}
fn chrs_to_str<'a>(chars: Vec<Chr>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(chars.iter()));
    Str::from_string(string)
}
fn str_order(left: Str, right: Str) -> Order {
    Order::from_ordering(left.cmp(&right))
}
fn str_walk_chrs_from<State, E>(
    str: Str,
    initial_state: State,
    on_element: impl Fn(State, Chr) -> Continue_or_exit<State, E>,
) -> Continue_or_exit<State, E> {
    Continue_or_exit::from_control_flow(std::iter::Iterator::try_fold(
        &mut str.as_str().chars(),
        initial_state,
        |state, element| on_element(state, element).to_control_flow(),
    ))
}
fn str_attach_chr<'a>(left: Str, right: Chr) -> Str<'a> {
    let mut string: std::string::String = left.into_string();
    string.push(right);
    Str::from_string(string)
}
fn str_attach<'a>(left: Str, right: Str) -> Str<'a> {
    let string: std::string::String = left.into_string();
    Str::from_string(string + right.as_str())
}
fn strs_flatten<'a>(vec_of_str: Vec<Str>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::map(vec_of_str.iter(), Str::as_str));
    Str::from_string(string)
}

/// Do not call `_.to_vec()` on it. Prefer `.into_vec()`
#[derive(Clone)]
pub enum Vec<'a, A> {
    Rc(std::rc::Rc<std::vec::Vec<A>>),
    Slice(&'a [A]),
}
impl<A: StillIntoOwned + Clone> StillIntoOwned for Vec<'_, A> {
    type Owned = std::vec::Vec<A::Owned>;
    fn into_owned(self) -> Self::Owned {
        match self {
            Vec::Rc(rc) => match std::rc::Rc::try_unwrap(rc) {
                std::result::Result::Ok(owned) => {
                    std::iter::Iterator::collect(std::iter::Iterator::map(
                        std::iter::IntoIterator::into_iter(owned),
                        A::into_owned,
                    ))
                }
                std::result::Result::Err(rc) => std::iter::Iterator::collect(
                    std::iter::Iterator::map(std::iter::Iterator::cloned(rc.iter()), A::into_owned),
                ),
            },
            Vec::Slice(slice) => std::iter::Iterator::collect(std::iter::Iterator::map(
                std::iter::Iterator::cloned(slice.iter()),
                A::into_owned,
            )),
        }
    }
    fn into_owned_overwriting(self, vec_allocation_to_reuse: &mut Self::Owned) {
        vec_allocation_to_reuse.clear();
        let vec_allocation_to_reuse_len: usize = vec_allocation_to_reuse.len();
        match self {
            Vec::Rc(rc) => match std::rc::Rc::try_unwrap(rc) {
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
            },
            Vec::Slice(slice) => {
                vec_allocation_to_reuse.truncate(slice.len());
                for (element_allocation_to_reuse, element) in
                    std::iter::Iterator::zip(vec_allocation_to_reuse.iter_mut(), slice.iter())
                {
                    A::into_owned_overwriting(element.clone(), element_allocation_to_reuse);
                }
                std::iter::Extend::extend(
                    vec_allocation_to_reuse,
                    std::iter::Iterator::map(
                        std::iter::Iterator::skip(slice.iter(), vec_allocation_to_reuse.len()),
                        |element| A::into_owned(element.clone()),
                    ),
                );
            }
        }
    }
}
impl<A: OwnedToStill> OwnedToStill for std::vec::Vec<A> {
    type Still<'a>
        = Vec<'a, A::Still<'a>>
    where
        A: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        Vec::from_vec(std::iter::Iterator::collect(std::iter::Iterator::map(
            self.iter(),
            |element_owned_ref| A::to_still(element_owned_ref, allocator),
        )))
    }
    fn into_still<'a>(self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        // is the optimizer smart enough to inline map if possible?
        Vec::from_vec(std::iter::Iterator::collect(std::iter::Iterator::map(
            std::iter::IntoIterator::into_iter(self),
            |element_owned| A::into_still(element_owned, allocator),
        )))
    }
}
impl<A: std::fmt::Debug> std::fmt::Debug for Vec<'_, A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Vec::Rc(rc) => std::fmt::Debug::fmt(rc, formatter),
            Vec::Slice(slice) => std::fmt::Debug::fmt(slice, formatter),
        }
    }
}
impl<A: Eq> Eq for Vec<'_, A> {}
impl<A: PartialEq> PartialEq for Vec<'_, A> {
    fn eq(&self, other: &Vec<A>) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}
impl<A> std::convert::AsRef<[A]> for Vec<'_, A> {
    fn as_ref(&self) -> &[A] {
        self.as_slice()
    }
}
impl<A> Vec<'_, A> {
    pub fn from_array<'a, const N: usize>(elements: [A; N]) -> Vec<'a, A> {
        Vec::from_vec(std::convert::Into::<std::vec::Vec<A>>::into(elements))
    }
    pub fn from_vec(vec: std::vec::Vec<A>) -> Self {
        Vec::Rc(std::rc::Rc::new(vec))
    }
    pub fn into_vec(self) -> std::vec::Vec<A>
    where
        A: Clone,
    {
        match self {
            Vec::Rc(rc) => std::rc::Rc::unwrap_or_clone(rc),
            Vec::Slice(slice) => slice.to_vec(),
        }
    }
    pub fn as_slice(&self) -> &[A] {
        match self {
            Vec::Rc(rc) => rc,
            Vec::Slice(slice) => slice,
        }
    }
    pub fn iter(&self) -> impl std::iter::Iterator<Item = &A> {
        self.as_slice().iter()
    }
}
fn vec_repeat<'a, A: Clone>(length: Unt, element: A) -> Vec<'a, A> {
    Vec::from_vec(std::iter::Iterator::collect(std::iter::repeat_n(
        element, length,
    )))
}
fn vec_length<A>(vec: Vec<A>) -> Unt {
    vec.as_slice().len()
}
fn vec_element<A: Clone>(vec: Vec<A>, index: Unt) -> Opt<A> {
    match vec.as_slice().get(index) {
        std::option::Option::None => Opt::Absent,
        std::option::Option::Some(element) => Opt::Present(element.clone()),
    }
}
fn vec_replace_element<A: Clone>(vec: Vec<A>, index: Unt, new_element: A) -> Vec<A> {
    if index >= vec.as_slice().len() {
        return vec;
    }
    let mut owned_vec: std::vec::Vec<A> = vec.into_vec();
    owned_vec[index] = new_element;
    Vec::from_vec(owned_vec)
}
fn vec_swap<A: Clone>(vec: Vec<A>, a_index: Unt, b_index: Unt) -> Vec<A> {
    if a_index >= vec.as_slice().len() || b_index >= vec.as_slice().len() || a_index == b_index {
        return vec;
    }
    let mut owned_vec: std::vec::Vec<A> = vec.into_vec();
    owned_vec.swap(a_index, b_index);
    Vec::from_vec(owned_vec)
}
fn vec_truncate<'a, A: 'a>(
    allocator: &'a impl Alloc,
    vec: Vec<'a, A>,
    taken_length: Unt,
) -> Vec<'a, A> {
    match vec {
        Vec::Rc(rc) => {
            if taken_length >= rc.len() {
                return Vec::Rc(rc);
            }
            match std::rc::Rc::try_unwrap(rc) {
                std::result::Result::Ok(mut owned_vec) => {
                    owned_vec.truncate(taken_length);
                    Vec::from_vec(owned_vec)
                }
                std::result::Result::Err(vec_rc) => {
                    Vec::Slice(allocator.alloc(vec_rc).get(..taken_length).unwrap_or(&[]))
                }
            }
        }
        Vec::Slice(slice) => Vec::Slice(slice.get(..taken_length).unwrap_or(slice)),
    }
}
fn vec_slice_from_index_with_length<'a, A>(
    allocator: &'a impl Alloc,
    vec: Vec<'a, A>,
    start_index: Unt,
    slice_length: Unt,
) -> Vec<'a, A> {
    let slice: &[A] = match vec {
        Vec::Rc(rc) => allocator.alloc(rc),
        Vec::Slice(slice) => slice,
    };
    Vec::Slice(
        slice
            .get(start_index..(start_index + slice_length))
            .unwrap_or(&[]),
    )
}
fn vec_increase_capacity_by<A: Clone>(vec: Vec<A>, capacity_increase: Unt) -> Vec<A> {
    let mut owned_vec: std::vec::Vec<A> = vec.into_vec();
    owned_vec.reserve(capacity_increase);
    Vec::from_vec(owned_vec)
}
fn vec_sort<A: Clone>(vec: Vec<A>, element_order: impl Fn(A, A) -> Order) -> Vec<A> {
    let mut owned_vec: std::vec::Vec<A> = vec.into_vec();
    owned_vec.sort_unstable_by(|a, b| element_order(a.clone(), b.clone()).to_ordering());
    Vec::from_vec(owned_vec)
}
fn vec_attach_element<'a, A: Clone>(left: Vec<A>, right_element: A) -> Vec<'a, A> {
    let mut combined: std::vec::Vec<A> = left.into_vec();
    combined.push(right_element);
    Vec::from_vec(combined)
}
fn vec_attach<'a, A: Clone>(left: Vec<A>, right: Vec<A>) -> Vec<'a, A> {
    let mut combined: std::vec::Vec<A> = left.into_vec();
    match right {
        Vec::Rc(right_rc) => match std::rc::Rc::try_unwrap(right_rc) {
            std::result::Result::Err(rc) => {
                combined.extend_from_slice(&rc);
            }
            std::result::Result::Ok(owned) => {
                std::iter::Extend::extend(&mut combined, owned);
            }
        },
        Vec::Slice(right_slice) => {
            combined.extend_from_slice(right_slice);
        }
    }
    Vec::from_vec(combined)
}
fn vec_flatten<'a, A: Clone>(vec_vec: Vec<Vec<A>>) -> Vec<'a, A> {
    Vec::from_vec(match vec_vec {
        Vec::Rc(vec_vec) => match std::rc::Rc::try_unwrap(vec_vec) {
            std::result::Result::Ok(vec_vec) => {
                let mut flattened: std::vec::Vec<A> = std::vec::Vec::new();
                for inner in vec_vec {
                    match inner {
                        Vec::Rc(inner) => match std::rc::Rc::try_unwrap(inner) {
                            std::result::Result::Ok(inner) => {
                                std::iter::Extend::extend(&mut flattened, inner);
                            }
                            std::result::Result::Err(inner) => {
                                flattened.extend_from_slice(&inner);
                            }
                        },
                        Vec::Slice(inner) => {
                            flattened.extend_from_slice(inner);
                        }
                    }
                }
                flattened
            }
            std::result::Result::Err(vec_vec) => {
                std::iter::Iterator::collect(std::iter::Iterator::cloned(
                    std::iter::Iterator::flat_map(vec_vec.iter(), Vec::iter),
                ))
            }
        },
        Vec::Slice(slice) => std::iter::Iterator::collect(std::iter::Iterator::cloned(
            std::iter::Iterator::flat_map(slice.iter(), Vec::iter),
        )),
    })
}
fn vec_walk_from<A: Clone, State, E>(
    vec: Vec<A>,
    state: State,
    on_element: impl Fn(State, A) -> Continue_or_exit<State, E>,
) -> Continue_or_exit<State, E> {
    match vec {
        Vec::Rc(vec) => match std::rc::Rc::try_unwrap(vec) {
            std::result::Result::Ok(vec) => {
                Continue_or_exit::from_control_flow(std::iter::Iterator::try_fold(
                    &mut std::iter::IntoIterator::into_iter(vec),
                    state,
                    |state, element| on_element(state, element).to_control_flow(),
                ))
            }
            std::result::Result::Err(vec) => {
                Continue_or_exit::from_control_flow(std::iter::Iterator::try_fold(
                    &mut std::iter::Iterator::cloned(vec.iter()),
                    state,
                    |state, element| on_element(state, element).to_control_flow(),
                ))
            }
        },
        Vec::Slice(slice) => Continue_or_exit::from_control_flow(std::iter::Iterator::try_fold(
            &mut std::iter::Iterator::cloned(slice.iter()),
            state,
            |state, element| on_element(state, element).to_control_flow(),
        )),
    }
}
