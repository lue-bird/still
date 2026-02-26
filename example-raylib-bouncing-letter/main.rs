mod lily;

fn main() {
    let window_width: i32 = 640;
    let window_height: i32 = 480;
    let (mut raylib_handle, raylib_thread) = raylib::init()
        .size(window_width, window_height)
        .title("lily ♥ raylib")
        .build();
    let mut lily_state: lily::State = lily::initial_state(lily::Window_height·window_width {
        window_width: window_width as lily::Dec,
        window_height: window_height as lily::Dec,
    });
    'main_loop: while !raylib_handle.window_should_close() {
        let interface = lily::interface(lily_state);
        let maybe_new_state: Option<lily::State> = interface
            .iter()
            .find_map(|io| handle_io(&mut raylib_handle, &raylib_thread, io));
        match maybe_new_state {
            None => {
                break 'main_loop;
            }
            Some(new_lily_state) => {
                lily_state = new_lily_state;
            }
        }
    }
}
fn handle_io(
    raylib_handle: &mut raylib::RaylibHandle,
    raylib_thread: &raylib::RaylibThread,
    interface: &lily::Io<lily::State>,
) -> Option<lily::State> {
    match interface {
        lily::Io::Display(to_write_elements) => {
            let mut draw_handle: raylib::prelude::RaylibDrawHandle =
                raylib_handle.begin_drawing(raylib_thread);
            raylib::drawing::RaylibDraw::clear_background(
                &mut draw_handle,
                raylib::color::Color::BLACK,
            );
            for to_write in to_write_elements.iter() {
                raylib::drawing::RaylibDraw::draw_text(
                    &mut draw_handle,
                    to_write.str.as_str(),
                    to_write.x as i32,
                    to_write.y as i32,
                    to_write.size as i32,
                    raylib::color::Color::WHITE,
                );
            }
            None
        }
        lily::Io::Key_pressed(on_pressed_char) => raylib_handle
            .get_key_pressed_number()
            .map(|key| on_pressed_char(key as lily::Unt)),
        lily::Io::Frame_passed(on_frame_passed) => {
            Some(on_frame_passed(raylib_handle.get_frame_time() as f64))
        }
    }
}
