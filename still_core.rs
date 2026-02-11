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
/// Notice how all _inner_ values are also converted,
/// making this operation more expensive than `to_owned`/`clone`
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
    if to_divide_by == 0 {
        0
    } else {
        to_divide / to_divide_by
    }
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
fn unt_to_str(allocator: &impl Alloc, unt: Unt) -> Str<'_> {
    allocator.alloc(std::format!("{}", unt))
}
fn str_to_unt(str: Str) -> Opt<Unt> {
    match str.parse::<Unt>() {
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
    if to_divide_by == 0 {
        0
    } else {
        to_divide / to_divide_by
    }
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

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ContinueOrExit<Continue, Exit> {
    Continue(Continue),
    Exit(Exit),
}
impl<Continue: StillIntoOwned + Clone, Exit: StillIntoOwned + Clone> StillIntoOwned
    for ContinueOrExit<Continue, Exit>
{
    type Owned = ContinueOrExit<Continue::Owned, Exit::Owned>;
    fn into_owned(self) -> Self::Owned {
        match self {
            ContinueOrExit::Continue(continue_) => {
                ContinueOrExit::Continue(Continue::into_owned(continue_))
            }
            ContinueOrExit::Exit(exit) => ContinueOrExit::Exit(Exit::into_owned(exit)),
        }
    }
    fn into_owned_overwriting(self, allocation_to_reuse: &mut Self::Owned) {
        match (self, allocation_to_reuse) {
            (
                ContinueOrExit::Continue(continue_),
                ContinueOrExit::Continue(continue_allocation_to_reuse),
            ) => {
                Continue::into_owned_overwriting(continue_, continue_allocation_to_reuse);
            }
            (ContinueOrExit::Exit(exit), ContinueOrExit::Exit(exit_allocation_to_reuse)) => {
                Exit::into_owned_overwriting(exit, exit_allocation_to_reuse);
            }
            (self_, allocation_to_reuse) => {
                *allocation_to_reuse = Self::into_owned(self_);
            }
        }
    }
}
impl<Continue: OwnedToStill, Exit: OwnedToStill> OwnedToStill for ContinueOrExit<Continue, Exit> {
    type Still<'a>
        = ContinueOrExit<Continue::Still<'a>, Exit::Still<'a>>
    where
        Continue: 'a,
        Exit: 'a;
    fn to_still<'a>(&'a self, allocator: &'a impl Alloc) -> Self::Still<'a> {
        match self {
            ContinueOrExit::Continue(continue_) => {
                ContinueOrExit::Continue(Continue::to_still(continue_, allocator))
            }
            ContinueOrExit::Exit(exit) => ContinueOrExit::Exit(Exit::to_still(exit, allocator)),
        }
    }
}
impl<Continue, Exit> ContinueOrExit<Continue, Exit> {
    fn to_control_flow(self) -> std::ops::ControlFlow<Exit, Continue> {
        match self {
            ContinueOrExit::Continue(continue_) => std::ops::ControlFlow::Continue(continue_),
            ContinueOrExit::Exit(exit) => std::ops::ControlFlow::Break(exit),
        }
    }
    fn from_control_flow(control_flow: std::ops::ControlFlow<Exit, Continue>) -> Self {
        match control_flow {
            std::ops::ControlFlow::Continue(continue_) => ContinueOrExit::Continue(continue_),
            std::ops::ControlFlow::Break(exit) => ContinueOrExit::Exit(exit),
        }
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

fn chr_byte_count(chr: Chr) -> Unt {
    chr.len_utf8()
}
fn chr_order(left: Chr, right: Chr) -> Order {
    Order::from_ordering(left.cmp(&right))
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

fn str_byte_count(str: Str) -> Unt {
    str.len()
}
fn str_chr_at_byte_index(str: Str, byte_index: Unt) -> Opt<Chr> {
    Opt::from_option(
        str.get(str.ceil_char_boundary(byte_index)..)
            .and_then(|chr_sub| std::iter::Iterator::next(&mut chr_sub.chars())),
    )
}
fn str_slice_from_byte_index_with_byte_length<'a>(
    str: Str<'a>,
    start_index: Unt,
    slice_byte_length: Unt,
) -> Str<'a> {
    str.get(
        str.floor_char_boundary(start_index)
            ..str.ceil_char_boundary(start_index + slice_byte_length),
    )
    .unwrap_or("")
}
fn str_to_chrs(str: Str) -> Vec<Chr> {
    std::rc::Rc::new(std::iter::Iterator::collect(str.chars()))
}
fn chrs_to_str<'a>(allocator: &'a impl Alloc, chars: Vec<Chr>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(chars.iter()));
    allocator.alloc(string)
}
fn str_order(left: Str, right: Str) -> Order {
    Order::from_ordering(left.cmp(right))
}
fn str_walk_chrs_from<Exit, State>(
    str: Str,
    initial_state: State,
    on_element: impl Fn(State, Chr) -> ContinueOrExit<State, Exit>,
) -> ContinueOrExit<State, Exit> {
    ContinueOrExit::from_control_flow(std::iter::Iterator::try_fold(
        &mut str.chars(),
        initial_state,
        |state, element| on_element(state, element).to_control_flow(),
    ))
}
fn strs_flatten<'a>(allocator: &'a impl Alloc, vec_of_str: Vec<Str>) -> Str<'a> {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(vec_of_str.iter()));
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
fn vec_repeat<A: Clone>(length: Unt, element: A) -> Vec<A> {
    std::rc::Rc::new(std::iter::Iterator::collect(std::iter::repeat_n(
        element, length,
    )))
}
fn vec_length<A>(vec: Vec<A>) -> Unt {
    vec.len()
}
fn vec_element<A: Clone>(vec: Vec<A>, index: Unt) -> Opt<A> {
    match vec.get(index) {
        std::option::Option::None => Opt::Absent,
        std::option::Option::Some(element) => Opt::Present(element.clone()),
    }
}
fn vec_take<A: Clone>(vec: Vec<A>, taken_length: Unt) -> Vec<A> {
    match std::rc::Rc::try_unwrap(vec) {
        std::result::Result::Ok(mut owned_vec) => {
            owned_vec.truncate(taken_length);
            std::rc::Rc::new(owned_vec)
        }
        std::result::Result::Err(vec_rc) => std::rc::Rc::new(
            vec_rc
                .get(..taken_length)
                .map(std::convert::Into::<std::vec::Vec<A>>::into)
                .unwrap_or_else(|| std::vec![]),
        ),
    }
}
fn vec_increase_capacity_by<A: Clone>(vec: Vec<A>, capacity_increase: Unt) -> Vec<A> {
    let mut owned_vec: std::vec::Vec<A> = std::rc::Rc::unwrap_or_clone(vec);
    owned_vec.reserve(capacity_increase);
    std::rc::Rc::new(owned_vec)
}
fn vec_sort<A: Clone>(vec: Vec<A>, element_order: impl Fn(A, A) -> Order) -> Vec<A> {
    let mut owned_vec: std::vec::Vec<A> = std::rc::Rc::unwrap_or_clone(vec);
    owned_vec.sort_unstable_by(|a, b| element_order(a.clone(), b.clone()).to_ordering());
    std::rc::Rc::new(owned_vec)
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
fn vec_walk_from<A: Clone, Exit, State>(
    vec: Vec<A>,
    state: State,
    on_element: impl Fn(State, A) -> ContinueOrExit<State, Exit>,
) -> ContinueOrExit<State, Exit> {
    match std::rc::Rc::try_unwrap(vec) {
        std::result::Result::Err(vec) => {
            ContinueOrExit::from_control_flow(std::iter::Iterator::try_fold(
                &mut std::iter::Iterator::cloned(vec.iter()),
                state,
                |state, element| on_element(state, element).to_control_flow(),
            ))
        }
        std::result::Result::Ok(vec) => {
            ContinueOrExit::from_control_flow(std::iter::Iterator::try_fold(
                &mut std::iter::IntoIterator::into_iter(vec),
                state,
                |state, element| on_element(state, element).to_control_flow(),
            ))
        }
    }
}
