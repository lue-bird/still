mod still;

fn main() {
    let allocator = bumpalo::Bump::new();
    println!("{}", still::greet(&allocator, "insert your name here"));
}
impl still::Alloc for bumpalo::Bump {
    fn alloc<A>(&self, value: A) -> &A {
        self.alloc(value)
    }
}
