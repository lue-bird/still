mod run;

fn main() {
    let mut allocator: bumpalo::Bump = bumpalo::Bump::new();
    // let mut still_state: Some_still_type::Owned;
    for name in ["Pedro", "me"] {
        println!("{}", run::greet(&allocator, run::Name· { name: name }));
        // let old_state_still: Some_still_type = still_state.to_still();
        // let updated_state_still: Some_still_type = run::increment(&allocator, old_state_still);
        // StillIntoOwned::into_owned_overwriting(updated_state_still, &mut into_owned_overwriting);
        allocator.reset();
    }
}
impl run::Alloc for bumpalo::Bump {
    fn alloc<A>(&self, value: A) -> &A {
        self.alloc(value)
    }
}
