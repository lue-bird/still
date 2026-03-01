for running
```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```
- https://github.com/thedodd/trunk

```bash
trunk watch
```
and
```bash
trunk serve
```
then open <http://localhost:8080/>.
Run `lily build` whenever you want to rebuild.

I've found running both `serve` and `watch` together really unreliable so you probably need to
manually restart from time to time :/
