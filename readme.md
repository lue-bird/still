very small, explicitly boring programming language that compiles to rust, heavily inspired by [elm](https://elm-lang.org/).
Just experimentation.

## maybe interesting deviations

- full type-inference considered not useful. Instead, each expression and pattern is always concretely typed, if necessary with an explicit annotation. So things like `(++) appendable -> appendable -> appendable`, `0 : number`, `[] : List any` are all not allowed, and e.g. `str-append \:str:a, :str:b -> :str:`, `0.0`, `:vec int:[]` are used instead.

  Having concrete types everywhere also makes type checking faster, generates better errors and makes transpiling to almost any language very easy (e.g. elm's polymorphic number operations or `let`s are generally hard to infer and represent nicely in other languages)

- no `|>`, infix operators, currying, modules

## hello world

```still
run \:uninitialized-or {}:_ -> :io {}:Standard-out-write "hello, world\n"
```

## echo in loop

```still
run \:uninitialized-or str:state-or-uninitialized ->
  let state
        case state-or-uninitialized of
        :uninitialized-or str:Uninitialized -> ""
        :uninitialized-or str:Initialized :str:initialized -> initialized
  :io str:Io-batch
    [ :io str:Standard-out-write
        (str-flatten [ ansi-clear-screen, state, "\nType a sentence to echo: " ])
    , :io str:Standard-in-read-line (\:str:line -> line)
    ]

ansi-clear-screen "\u{001B}c"
```

## cons-list

```still
type stack A = Empty | Cons { head A, tail stack A }

stack-map :\A -> B:element-change, :stack A:stack ->
  case stack of
  :stack A:Empty -> :stack B:Empty
  :stack A:Cons { head :A:head, tail :stack A:tail } ->
    :stack B:Cons
      { head element-change head
      , tail stack-map element-change tail
      }
```

## TODO
- revert lambda only taking one parameter
- change comment system to `Expression::WithComment` and `Pattern::WithComment` and `Type::WithComment` (each meaning it is prefixed by `#`) and _always_ preserve line-spread of original range! Then, remove all &comments parameters
- type checking (notably also: check that each function output type only ever uses type variables used in the input type, and similarly: on non-function types, forbid the use of any new variables)
- `still build`
- small standard library in rust (`str` (&str), `vec` (Rc<Vec<>>), `int` (i32), `dec` (f32), ?`order`, ?`char`(unicode-scalar/rune), `int-to-str`, `dec-to-str`, `int/dec-add`, `int/dec-multiply`, `dec-power`, `str-compare`, `int-compare`, `dec-compare`, ...)
- simple io (`standard-in-read-line`, `standard-out-write`, ?`type uninitialized-or Initialized = Uninitialized | Initialized Initialized`)
- `case of` exhaustiveness checking
- unused checking
- name collision checking
- show errors and warning in lsp

## considering
- (leaning towards yes) actually deeply consider limiting reference calls to at most 1 argument just like variant construction.
  That would still not eliminate the need for parens in general (see lambda and case of) but allow e.g. `html-text int-to-str half window-width`
- adding anonymous choice types. They are not allowed to be recursive. Use `type alias` for these. choice types can then be removed
- find better function call syntax that makes it easy to unwrap the last argument
- find better string literal syntax, like zig's `//` or js' `\`\``
- (leaning no, at least for now) add or pattern `( first | second | third )` (potentially allow `:overall:( A | B | C )` (where the inner variant patterns don't need a type) specifically for variant)
- introduce `nat` type and require regular ints to be prefixed with `+`/`-`
- make formatter range independent, and instead cut a line >=100 (is that possible to do when trying to get a maximally fast formatter? Because it seems an intermediate recursive structure is required)
- output rust on save
- support out-of-order let declarations
- closed lambda, call and case-of syntax like `\pattern -> expression/`, `call<arg`, `if x ( A -> x | B -> y )`, then remove ::Parenthesized

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
- reparse incrementally (somewhat easy to implement but somehow it's for me at least pretty much fast enough already without? More data points welcome)
- switch to `position_encoding: Some(lsp_types::PositionEncodingKind::UTF8)`. This makes source edits and parsing easier and faster at the cost of compatibility with lsp clients below version 3.17.0. Is that acceptable? (leaning towards yes).
- if memory consumptions turns out to be a problem, stop storing the source in memory
  and request full file content on each change (potentially only for dependencies).
  This adds complexity and is slower so only if necessary.
- in syntax tree, use separate range type for single-line tokens like keywords, symbols, names etc to save on memory consumption
- in syntax tree, use `Box<[]>` instead of `Vec` for common nodes like call arguments
