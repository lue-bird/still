mod still;

fn main() {
    let mut allocator: bumpalo::Bump = bumpalo::Bump::new();
    let mut still_state: still::Opt<<StillState<'static> as still::StillIntoOwned>::Owned> =
        still::Opt::Absent;
    'main_loop: for _ in std::iter::repeat_n((), 10) {
        let interface = still::interface(
            &allocator,
            still::OwnedToStill::to_still(&still_state, &allocator),
        );
        let maybe_new_state: Option<StillState> = handle_io(&allocator, interface);
        match maybe_new_state {
            None => {
                break 'main_loop;
            }
            Some(new_still_state) => {
                still_state =
                    still::Opt::Present(still::StillIntoOwned::into_owned(new_still_state));
            }
        }
        allocator.reset();
    }
}
/// change this when you introduce a type alias in still or otherwise change its state type
type StillState<'a> = still::Str<'a>;
/// returns a new state
fn handle_io<'a>(
    allocator: &'a bumpalo::Bump,
    interface: still::Io<'a, still::Str<'a>>,
) -> Option<still::Str<'a>> {
    match interface {
        still::Io::Standard_out_write(to_write) => {
            print!("{}", to_write);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            None
        }
        still::Io::Batch(ios) => {
            for io in ios.iter().copied() {
                if let Some(new_state) = handle_io(allocator, io) {
                    return Some(new_state);
                }
            }
            None
        }
        still::Io::Standard_in_read_line(on_read_line) => {
            let mut read_line: String = String::new();
            let _ = std::io::stdin().read_line(&mut read_line);
            Some(on_read_line(allocator.alloc(read_line)))
        }
    }
}
impl still::Alloc for bumpalo::Bump {
    fn alloc<A>(&self, value: A) -> &A {
        self.alloc(value)
    }
}
