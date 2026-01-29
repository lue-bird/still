mod run;

fn main() {
    let mut allocator: bumpalo::Bump = bumpalo::Bump::new();
    // let mut still_state: Some_still_type::StillToOwned;
    for _ in std::iter::repeat_n((), 10) {
        println!("{}", run::greet(&allocator, "me"));
        // let old_state_still: Some_still_type = OwnedToStill::to_still(still_state);
        // let updated_state_still: Some_still_type = run::increment(&allocator, old_state_still);
        // still_state = StillToOwned::to_owned(updated_state_still);
        allocator.reset();
    }
}
impl run::Alloc for bumpalo::Bump {
    fn alloc<A>(&self, value: A) -> &A {
        self.alloc(value)
    }
}
