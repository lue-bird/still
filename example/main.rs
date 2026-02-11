mod still;

fn main() {
    let mut allocator: bumpalo::Bump = bumpalo::Bump::new();
    let mut still_state = still::INITIAL_STATE;
    for _ in std::iter::repeat_n((), 10) {
        let updated_state_still = still::interface(
            &allocator,
            still::OwnedToStill::to_still(&still_state, &allocator),
        );
        still::StillIntoOwned::into_owned_overwriting(
            updated_state_still.new_state,
            &mut still_state,
        );
        println!("{}", updated_state_still.standard_out_write);
        allocator.reset();
    }
}
impl still::Alloc for bumpalo::Bump {
    fn alloc<A>(&self, value: A) -> &A {
        self.alloc(value)
    }
}
