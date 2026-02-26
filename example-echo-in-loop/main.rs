// enabling deref_patterns is sadly required for matching recursive choice types
#![feature(deref_patterns)]
#![allow(incomplete_features)]

mod lily;

/// you'll most likely want to introduce an alias for this on the lily side instead
type LilyState = lily::Str;

fn main() {
    let mut lily_state: lily::Opt<LilyState> = lily::Opt::Absent;
    'main_loop: loop {
        let interface = lily::interface(lily_state);
        let maybe_new_state: Option<LilyState> = interface.iter().find_map(handle_io);
        match maybe_new_state {
            None => {
                break 'main_loop;
            }
            Some(new_lily_state) => {
                lily_state = lily::Opt::Present(new_lily_state);
            }
        }
    }
}
/// returns a new state
fn handle_io(io: &lily::Io<LilyState>) -> Option<LilyState> {
    match io {
        lily::Io::Standard_out_write(to_write) => {
            print!("{}", to_write);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            None
        }
        lily::Io::Standard_in_read_line(on_read_line) => {
            let mut read_line: String = String::new();
            let _ = std::io::stdin().read_line(&mut read_line);
            Some(on_read_line(lily::Str::from_string(read_line)))
        }
    }
}
