very small, explicitly boring programming language that compiles to rust, inspired by [elm](https://elm-lang.org/).
> ⚠️ Experimental, subject to change, use with caution.

### hello world

```still
run \:opt {}:_ >
    :io {}:Standard-out-write "hello, world\n"
```
the syntactically equivalent elm code would be
```elm
run : Maybe {} -> Io {}
run = \_ ->
    StandardOutWrite "hello, world\n"
```

### echo in loop

```still
ansi-clear-screen "\u{001B}c"

run \:opt str:state-or-uninitialized >
    let state
        case state-or-uninitialized
        | :opt str:Absent > ""
        | :opt str:Present :str:initialized > initialized
    :io str:Batch
        [ :io str:Standard-out-write
            strs-flatten [ ansi-clear-screen, state, "\nType a sentence to echo: " ]
        , :io str:Standard-in-read-line \:str:line > line
        ]
```

To use, [install rust](https://rust-lang.org/tools/install/) and
```bash
cargo +nightly install --git https://github.com/lue-bird/still
```
Then point your editor to `still lsp`, see also [specific setups](#editor-setups).

## maybe interesting

- each expression and pattern is always concretely typed, if necessary with an explicit annotation. So things like `(++) appendable -> appendable -> appendable`, `0 : number`, `[] : List any` are all not allowed, and e.g. `str-append \:str:l, :str:r > :str:`, `0.0`, `:vec int:[]` are used instead.

  → Faster type checking, clear errors, easy compilation to almost any language

- no blocking compile errors. You can always build, even if your record is still missing a field value, your matching is still inexhaustive, some parens are empty, etc.
  You will still see all the errors, though.

- io and memory is handled in steps.
  Each step builds new io from the current state (the io also specifies how to build new state based on events).
  During that step, any function anywhere can liberally allocate memory as needed.
  After that step, the updated state is cloned into a loop-global variable and the allocator containing all the memory allocated in this step is reset.
  See [`example/`](/example/)

  Requiring diffing of the state and deep conversions alone disqualifies this memory model for performance-critical programs. It should however be competitive for regular applications which tend to have simple state but a bunch of memory waste at each frame/update/...

- no `Task`/`async`, visible mutation, side effects, infix operators, currying, modules, lifetime tracking

## TODO
- remove let destructuring
- correct impl to &dyn in type declarations (is impl not supported in type alias?)
- correctly clone captures before closure
- avoid generating unused lifetime in fn item when no allocator and its type uses lifetime
- fix bug of `\n` being printed as `\\n`
- type checking (vec elements equal, case results equal, function arguments equal to parameters, typed, variant value) (notably also: check that each function output type only ever uses type variables used in the input type, and similarly: on non-function types, forbid the use of any new variables; in the error say "unknown type variable")
- complete small standard library in rust (TODO `order`, `dec-power`, `str-compare`, `int-compare`, `dec-compare`, `map`, `set`, ...)
- replace `&'a dyn Fn(_) -> _` in function parameters by `impl Fn(_) -> _ + Clone + 'a`
  and likewise remove `alloc.alloc(|_| _)` when used as direct function parameter: `|_| _`
- introduce `nat` type (`usize`) and require regular ints to be prefixed with `+`/`-`
- simple io (`standard-in-read-line`, `standard-out-write`)
- `case of` exhaustiveness checking
- unused checking
- name collision checking
- name shadowing checking
- implement `StillIntoOwned::into_owned_overwriting` for generated structs and enums

## considering
- adding anonymous choice types. They are not allowed to be recursive. Use `type alias` for these. choice types can then be removed. Should be fairly easy to implement but potentially not that nice for FFI, similar to record structs currently
- find better string literal syntax, like zig's `//` or js' `\`\``
- (leaning no, at least for now) add or pattern `( first | second | third )` (potentially allow `:overall:( A | B | C )` (where the inner variant patterns don't need a type) specifically for variant)
- make formatter range-independent, and instead cut a line >=100 (is that possible to do when trying to get a maximally fast formatter? Because it seems an intermediate recursive structure is required)
- output rust in realtime. Really cool since the compiled code is always up to date, need to check if file io is fast enough
- (leaning towards no) extend typing model to only specify type variables, so `myFunction<int, str>`, `[]<int>`, `Present<int> 1`, similar to dhall and zig (but worse, because not first class. If it was you could pass types in records etc).

  ```still
  stack-map<A, B> \:\A > B:element-change, :stack<A>:stack >
      case stack of
      Empty<A> > Empty<B>
      Cons<A> { head :A:head, tail :stack<A>:tail } >
          Cons<B>
              { head element-change head
              , tail stack-map<A, B> element-change tail
              }
  ```
  This generally removes some verbosity, is consistent with choice type/ type alias construction,
  allows non-called generic functions, would allow the removal of all "::Typed" patterns and expressions (except recursion? but maybe there is a better solution for that).
- (seems completely useless) infer constness of generated variable/fn items

## syntax overview
### matching and destructuring
Any expression can be followed with any number of `| pattern > result` cases:
```still
option
| :opt int:Absent >
    0
| :opt int:Present n >
    n + 1
```
The last case result is allowed to be unindented;
in effect this is like an early return.

This indentation trick makes it fairly nice to do simple destructuring:
```still
variant
| :some:Variant member >
result
```
or something close to pipelines
```still
f x argument
| :f-result:f-result >
g y first-result
| :g-result:g-result >
h z g-result
```
You will probably prefer `let` for most cases, though.

## editor setups
feel free to contribute as I only use vscodium

### vscode-like
#### pre-built
1. download https://github.com/lue-bird/still/blob/main/vscode/still-0.0.1.vsix
2. open the command bar at the top and select: `>Extensions: Install from VSIX`
#### build from source
1. clone this repo
2. open `vscode/`
3. run `npm run package` to create the `.vsix`
4. open the command bar at the top and select: `>Extensions: Install from VSIX`
#### server only
There is no built-in language server bridge as far as I know but you can install an extension like [vscode-generic-lsp-proxy](https://github.com/mjmorales/vscode-generic-lsp-proxy) that will work for any language server.
Then add a `.vscode/lsp-proxy.json` like
```json
[
  {
    "languageId": "still",
    "command": "still",
    "fileExtensions": [
      ".still"
    ]
  }
]
```

### helix
write to `~/.config/helix/languages.toml`:
```toml
[language-server.still]
command = "still lsp"
[[language]]
name = "still"
scope = "source.still"
injection-regex = "still"
file-types = ["still"]
indent = { tab-width = 2, unit = "  " }
language-servers = [ "still" ]
auto-format = true
```

## setup for developing
Rebuild the project with
```bash
cargo build
```
Then point your editor to the created `???/target/debug/still lsp`.

### log of failed optimizations
- switching to mimalloc, ~>25% faster (really nice) at the cost of 25% more memory consumption.
  Might be worth for some people but I'm already worried about our memory footprint!
- `declarations.shrink_to_fit();` saves around 0.6% of memory at the cost of a bit of speed
- upgrading `lto` to `"thin"` to `"fat"` both improve runtime speed by ~13% compared to the default (and reduce binary size) but increase build time by about 30% (default to thin) and 15% (thin to fat).
  As this prolongs installation and prevents people from quickly trying it, the default is kept.
  If this language server get distributed as a binary or people end up using this language server a lot, this `"thin"` might become a reasonable trade-off.

### optimizations to try
- reimplement [strongly_connected_components](https://docs.rs/strongly-connected-components/latest/strongly_connected_components/) myself
- reparse incrementally (somewhat easy to implement but somehow it's for me at least pretty much fast enough already without? More data points welcome)
- switch to `position_encoding: Some(lsp_types::PositionEncodingKind::UTF8)`. This makes source edits and parsing easier and faster at the cost of compatibility with lsp clients below version 3.17.0. Is that acceptable? (leaning towards yes).
- if memory consumptions turns out to be a problem, stop storing the source in memory
  and request full file content on each change (potentially only for dependencies).
  This adds complexity and is slower so only if necessary.
- in syntax tree, use separate range type for single-line tokens like keywords, symbols, names etc to save on memory consumption
- in syntax tree, use `Box<[]>` instead of `Vec` for common nodes like call arguments
