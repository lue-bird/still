#![no_implicit_prelude]
#![allow(
    dead_code,
    non_shorthand_field_patterns,
    non_camel_case_types,
    clippy::needless_pass_by_value,
    clippy::wrong_self_convention
)]
extern crate std;
use std::clone::Clone;
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;
use std::marker::Copy;
use std::ops::Fn;
// core //

fn closure_rc<A, B>(closure: impl Fn(A) -> B + 'static) -> std::rc::Rc<dyn Fn(A) -> B> {
    std::rc::Rc::new(closure)
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Debug)]
pub struct Blank {}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Debug)]
pub enum Order {
    Less = -1,
    Equal = 0,
    Greater = 1,
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
fn unt_to_str(unt: Unt) -> Str {
    Str::from_string(std::format!("{}", unt))
}
fn str_to_unt(str: Str) -> Opt<Unt> {
    match str.as_str().parse::<Unt>() {
        std::result::Result::Err(_) => Opt::Absent,
        std::result::Result::Ok(unt) => Opt::Present(unt),
    }
}

pub type Int = isize;

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
fn int_to_str(int: Int) -> Str {
    Str::from_string(std::format!("{}", int))
}
fn str_to_int(str: Str) -> Opt<Int> {
    match str.as_str().parse::<Int>() {
        std::result::Result::Err(_) => Opt::Absent,
        std::result::Result::Ok(int) => Opt::Present(int),
    }
}

pub type Dec = f32;

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
fn dec_to_str(dec: Dec) -> Str {
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
fn chr_to_str(chr: Chr) -> Str {
    Str::from_string(std::format!("{}", chr))
}
/// prefer Str::into_string over Str::to_string
#[derive(Clone)]
pub enum Str {
    Rc(std::rc::Rc<std::string::String>),
    Slice(&'static str),
}
impl std::convert::AsRef<str> for Str {
    fn as_ref(&self) -> &'_ str {
        match self {
            Str::Rc(rc) => rc,
            Str::Slice(slice) => slice,
        }
    }
}
impl std::fmt::Debug for Str {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), formatter)
    }
}
impl std::fmt::Display for Str {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(self.as_str(), formatter)
    }
}
impl Str {
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
impl Eq for Str {}
impl PartialEq for Str {
    fn eq(&self, other: &Str) -> bool {
        self.as_str() == other.as_str()
    }
}
impl PartialEq<str> for Str {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl Ord for Str {
    fn cmp(&self, other: &Str) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl PartialOrd for Str {
    fn partial_cmp(&self, other: &Str) -> std::option::Option<std::cmp::Ordering> {
        std::option::Option::Some(self.cmp(other))
    }
}
impl PartialOrd<str> for Str {
    fn partial_cmp(&self, other: &str) -> std::option::Option<std::cmp::Ordering> {
        std::option::Option::Some(self.as_str().cmp(other))
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
fn str_slice_from_byte_index_with_byte_length(
    str: Str,
    start_index: Unt,
    slice_byte_length: Unt,
) -> Str {
    match str {
        Str::Slice(slice) => Str::Slice(
            slice
                .get(
                    slice.floor_char_boundary(start_index)
                        ..slice.ceil_char_boundary(start_index + slice_byte_length),
                )
                .unwrap_or(""),
        ),
        Str::Rc(rc) => rc
            .get(
                rc.floor_char_boundary(start_index)
                    ..rc.ceil_char_boundary(start_index + slice_byte_length),
            )
            .map(|slice| Str::from_string(std::string::ToString::to_string(slice)))
            .unwrap_or(Str::Slice("")),
    }
}
fn str_to_chrs(str: Str) -> Vec<Chr> {
    Vec::from_vec(std::iter::Iterator::collect(str.as_str().chars()))
}
fn chrs_to_str(chars: Vec<Chr>) -> Str {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::copied(chars.iter()));
    Str::from_string(string)
}
fn str_order(left: Str, right: Str) -> Order {
    Order::from_ordering(left.cmp(&right))
}
fn str_walk_chrs_from<C, E>(
    str: Str,
    initial_state: C,
    on_element: impl Fn(C, Chr) -> Continue_or_exit<C, E>,
) -> Continue_or_exit<C, E> {
    Continue_or_exit::from_control_flow(std::iter::Iterator::try_fold(
        &mut str.as_str().chars(),
        initial_state,
        |state, element| on_element(state, element).to_control_flow(),
    ))
}
fn str_attach_chr(left: Str, right: Chr) -> Str {
    let mut string: std::string::String = left.into_string();
    string.push(right);
    Str::from_string(string)
}
fn str_attach(left: Str, right: Str) -> Str {
    let string: std::string::String = left.into_string();
    Str::from_string(string + right.as_str())
}
fn strs_flatten(vec_of_str: Vec<Str>) -> Str {
    let string: std::string::String =
        std::iter::Iterator::collect(std::iter::Iterator::map(vec_of_str.iter(), Str::as_str));
    Str::from_string(string)
}

/// Do not call `_.to_vec()` on it. Prefer `.into_vec()`
#[derive(Clone)]
pub enum Vec<A> {
    Rc(std::rc::Rc<std::vec::Vec<A>>),
}
impl<A: std::fmt::Debug> std::fmt::Debug for Vec<A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Vec::Rc(rc) => std::fmt::Debug::fmt(rc, formatter),
        }
    }
}
impl<A: Eq> Eq for Vec<A> {}
impl<A: PartialEq> PartialEq for Vec<A> {
    fn eq(&self, other: &Vec<A>) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}
impl<A> std::convert::AsRef<[A]> for Vec<A> {
    fn as_ref(&self) -> &[A] {
        self.as_slice()
    }
}
impl<A> Vec<A> {
    pub fn from_array<const N: usize>(elements: [A; N]) -> Vec<A> {
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
        }
    }
    pub fn as_slice(&self) -> &[A] {
        match self {
            Vec::Rc(rc) => rc,
        }
    }
    pub fn iter(&self) -> impl std::iter::Iterator<Item = &A> {
        self.as_slice().iter()
    }
}
fn vec_repeat<A: Clone>(length: Unt, element: A) -> Vec<A> {
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
fn vec_truncate<A: Clone>(vec: Vec<A>, taken_length: Unt) -> Vec<A> {
    match vec {
        Vec::Rc(rc) => {
            if taken_length >= rc.len() {
                return Vec::Rc(rc);
            }
            let mut owned_vec: std::vec::Vec<A> = std::rc::Rc::unwrap_or_clone(rc);
            owned_vec.truncate(taken_length);
            Vec::from_vec(owned_vec)
        }
    }
}
fn vec_slice_from_index_with_length<A: Clone>(
    vec: Vec<A>,
    start_index: Unt,
    slice_length: Unt,
) -> Vec<A> {
    match vec {
        Vec::Rc(rc) => {
            if start_index >= rc.len() {
                return Vec::from_array([]);
            }
            let slice_range: std::ops::Range<usize> =
                start_index..(start_index + slice_length).max(rc.len());
            match std::rc::Rc::try_unwrap(rc) {
                std::result::Result::Ok(mut owned_vec) => Vec::from_vec(
                    std::iter::Iterator::collect::<std::vec::Vec<_>>(owned_vec.drain(slice_range)),
                ),
                std::result::Result::Err(rc) => rc
                    .get(slice_range)
                    .map(|slice: &[A]| Vec::from_vec(slice.to_vec()))
                    .unwrap_or_else(|| Vec::from_array([])),
            }
        }
    }
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
fn vec_attach_element<A: Clone>(left: Vec<A>, right_element: A) -> Vec<A> {
    let mut combined: std::vec::Vec<A> = left.into_vec();
    combined.push(right_element);
    Vec::from_vec(combined)
}
fn vec_attach<A: Clone>(left: Vec<A>, right: Vec<A>) -> Vec<A> {
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
    }
    Vec::from_vec(combined)
}
fn vec_flatten<A: Clone>(vec_vec: Vec<Vec<A>>) -> Vec<A> {
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
    })
}
fn vec_walk_from<A: Clone, C, E>(
    vec: Vec<A>,
    state: C,
    on_element: impl Fn(C, A) -> Continue_or_exit<C, E>,
) -> Continue_or_exit<C, E> {
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
    }
}
