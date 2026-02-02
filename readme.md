very small, explicitly boring programming language that compiles to rust, inspired by [elm](https://elm-lang.org/).
> ⚠️ Just experimentation, use with a bucket of caution and salt.

### hello world

```still
run \:opt {}:_ >
    :io {}:Standard-out-write "hello, world\n"
```
the equivalent elm code would be
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
          case state-or-uninitialized of
          :opt str:Absent > ""
          :opt str:Present :str:initialized > initialized
    :io str:Io-batch
        [ :io str:Standard-out-write
            (str-flatten [ ansi-clear-screen, state, "\nType a sentence to echo: " ])
        , :io str:Standard-in-read-line (\:str:line > line)
        ]
```

## maybe interesting

- each expression and pattern is always concretely typed, if necessary with an explicit annotation. So things like `(++) appendable -> appendable -> appendable`, `0 : number`, `[] : List any` are all not allowed, and e.g. `str-append \:str:l, :str:r > :str:`, `0.0`, `:vec int:[]` are used instead.

  → Faster type checking, precise errors, easy compilation to almost any language

- no blocking compile errors. You can always build, even if your record is still missing a field value, your matching is still inexhaustive, some parens are empty, etc.
  You will still see all the errors, though.

- io and memory is handled in steps.
  Each step builds new io from the current state (the io also specifies how to build new state based on events).
  During that step, any function anywhere can liberally allocate memory as needed.
  After that step, the updated state is cloned into a loop-global variable and the allocator containing all the memory allocated in this step is reset.
  See [`example/`](/example/)

  Requiring clones of the state alone disqualifies this memory model for performance-critical programs. It should however be competitive for regular applications which tend to have simple state but a bunch of memory waste at each frame/update/...

- no `Task`/`async`, detectable mutation, side effects, `|>`, infix operators, currying, modules, lifetime tracking

## TODO
- generate implementations for `StillToOwned` and `OwnedToStill` for generated choice types
- rename `case of` to `if x = ... > ... = ... > ...` and remove let destructuring. previous:
  ```still
  let :some:Variant member = variant
  result
  #
  case option of
  :opt int:Absent >
    0
  :opt int:Present n >
    n + 1
  ```
  now
  ```still
  if variant = :some:Variant member >
  result
  #
  if option
  = :opt int:Absent >
    0
  = :opt int:Present n >
  n + 1
  ```
  this solves the nesting problem of early exits and "if else"
- complete small standard library in rust (TODO `order`, `int/dec-add`, `int/dec-multiply`, `dec-power`, `str-compare`, `int-compare`, `dec-compare`, `map`, `set`, `type opt A = Absent | Present A` ...)
- For choice type with recursive variant values, introduce an owned version which uses Box
- type checking (vec elements equal, case results equal, function arguments equal to parameters, typed, variant value) (notably also: check that each function output type only ever uses type variables used in the input type, and similarly: on non-function types, forbid the use of any new variables; in the error say "unknown type variable")
- rename `type` to `type-choice` and `type alias` to `type-alias`
- replace `&'a dyn Fn(_) -> _` in function parameters by `impl Fn(_) -> _ + Clone + 'a`
  and likewise remove `alloc.alloc(|_| _)` when used as direct function parameter: `|_| _`
- introduce `nat` type (`usize`) and require regular ints to be prefixed with `+`/`-`
- simple io (`standard-in-read-line`, `standard-out-write`)
- `case of` exhaustiveness checking
- unused checking
- name collision checking
- name shadowing checking

## considering
- (leaning towards yes) actually deeply consider limiting reference calls to at most 1 argument just like variant construction.
  That would still not eliminate the need for parens in general (see lambda and case of) but allow e.g. `html-text int-to-str half window-width`
- adding anonymous choice types. They are not allowed to be recursive. Use `type alias` for these. choice types can then be removed
- find better function call syntax that makes it easy to unwrap the last argument
- find better string literal syntax, like zig's `//` or js' `\`\``
- (leaning no, at least for now) add or pattern `( first | second | third )` (potentially allow `:overall:( A | B | C )` (where the inner variant patterns don't need a type) specifically for variant)
- make formatter range independent, and instead cut a line >=100 (is that possible to do when trying to get a maximally fast formatter? Because it seems an intermediate recursive structure is required)
- output rust on save
- closed lambda, call and case-of syntax like `\pattern > expression/`, `call<arg`, `if x ( A > x | B > y )`, then remove ::Parenthesized
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
- infer constness of generated variable/fn items
- find better record access syntax, like `:field-value:.field record`

To use, [install rust](https://rust-lang.org/tools/install/) and
```bash
cargo +nightly install --git https://github.com/lue-bird/still
```
Then point your editor to `still lsp`, see also [specific setups](#editor-setups).

## editor setups
feel free to contribute, as I only use vscodium

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
