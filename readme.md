very small, explicitly boring, purely functional programming language that compiles to rust, inspired by [elm](https://elm-lang.org/).
> ready for small projects, not for production

```lily
greet \:str:name >
    strs-flatten [ "Hello, ", name, "\n" ]
```
Variables don't perform any effects.
The compiled code can however be used from rust to actually do something:
```rust
mod lily;
fn main() {
    print!("{}", lily::greet(lily::Str::Slice("insert your name here")));
}
```
→ complete setup: [`example-hello-world/`](https://github.com/lue-bird/lily/tree/main/example-hello-world)

### bigger example: echo in loop
```lily
ansi-clear-screen
    "\u{001B}c"

interface \:opt str:state-or-uninitialized >
    let state
        state-or-uninitialized
        | :opt str:Absent > ""
        | :opt str:Present :str:initialized > initialized
    [ :io str:Standard-out-write
        strs-flatten [ ansi-clear-screen, state, "\nType a sentence to echo: " ]
    , :io str:Standard-in-read-line \:str:line > line
    ]

choice io Future
    | Standard-out-write str
    | Standard-in-read-line \str > Future
```
→ [`example-echo-in-loop/`](https://github.com/lue-bird/lily/tree/main/example-echo-in-loop), for syntax questions see [the syntax overview](#syntax-overview)

To use, [install rust](https://rust-lang.org/tools/install/) and
```bash
cargo +nightly install --git https://github.com/lue-bird/lily lily
```
Then point your editor to `lily lsp`, see also [specific setups](#editor-setups).

## maybe interesting

- each expression and pattern is always concretely typed, if necessary with an explicit annotation. So things like `(++) appendable -> appendable -> appendable`, `0 : number`, `[] : List any` are all not allowed, and e.g. `str-attach \:str:l, :str:r > :str:`, `0.0`, `:vec int:[]` are used instead.

  → faster type checking, clear errors, a few less bugs, easy compilation to almost any language

- no blocking compile errors. You can always build, even if your record is still missing a field value, your matching is still inexhaustive, some parens are empty, etc.
  You will still see all the errors, though.

- no features that obfuscate ("shiny, cool features" that ruin languages in my opinion): infix operators, currying, traits/type classes/overloading, objects, task/async, hidden mutation, macros & reflection, lifetime tracking, hidden side effects, modules, hidden context values, exceptions, undefined

## syntax overview
```lily
# this is a comment.

# declared variable, type and field names use ascii letters, digits and -
s0me-Name

# any expression/pattern can have an explicit :type:
:str:some-variable-name

# string (of type str)
"Yahallo"

# multi-line string (each line starts with `, then raw text until linebreak)
`Ma! I got a thing going here.
`You got lint on your fuzz.
`Ow! That's me!
`Wave to us! "\\\ ` ' \n \r \t \{ \m \u}

# character (of type char)
'👀'

# signed integer (of type int, sign is required)
+2012

# unsigned integers (each of type unt, no sign)
2012
0

# signed integer zero (of type int, sign is required even for 0 → 00)
00

# floating point number (of type dec, sign is optional)
1.25

# function call (with result type int)
int-add -2 +3

# list expression with elements of the same type (of type vec unt)
[ 1, 2, 3 ]

# empty vec (:explicit type: is required)
:vec int:[]

# a bunch of labelled values grouped together
#   (of type { likes unt, dislikes unt, boosts unt })
{ likes 1, dislikes unt-add 1 2, boosts 3 }

# local variable declaration (must be in order and not recursive)
= local-variable-name "Anissa"
strs-flatten [ "Hello, ", local-variable-name, "\n" ]

# an abbreviation for a commonly used type
type point Unity-type-parameter =
    { x Unity-type-parameter, y Unity-type-parameter }

# for expressions that are either one thing or some another thing
choice card Custom-joker-action
    | Draw4
    | Joker
        # variants can have 0 or 1 value
        Custom-joker-action
    | Regular
        { color color
        , value unt
        }

# variant (:type: is required)
:card unt:Joker 1

# function (the first symbol is a backslash)
\first-pattern, second-pattern > result-expression

# a pattern can be a number, string, character, record, variant or
#     a variable (:type: is required)
:str:incoming-string
#     a wildcard: match anything and discard (:type: is required)
:card unt:_

# for different cases of how a value looks, exhaustively decide what to do
# in the example below: given a leftover card, assign minus points
card
| :card unt:Draw4 >
    40
| :card unt:Joker 0 >
    20
| :card unt:Joker :unt:joker_power >
    unt-mul joker_power 5
| :card unt:Regular { color :color:_, value :unt:value } >
    value

# The last case result is allowed to be unindented;
# in effect this is like an early return.
# This indentation trick makes it fairly nice to do simple destructuring:
variant
| :some:Variant member >
result

# or something close to pipelines
# You will probably prefer local variables (= name ...) in most cases, though.
f x argument
| :f-result:f-result >
g y first-result
| :g-result:g-result >
h z g-result

# suffixing a local variable with ^ shadows a previous variable (also in patterns)
# This is often used in situations similar to where you'd typically
# use mutation/pipelines in other languages,
# for example builders, random seed state or parse state
= s "("
= s^ str-attach-char s ' '
= s^ str-attach-unt s 10
= s^ str-attach s " > "
str-attach-dec s 0.2
```
That should be all. If not, the examples may show more.

## questions you might have

### how is memory managed
Regular types are passed by value, copying if necessary.
`vec`, `str`, recursive variant values and closures however can be reference-counted,
so passing structures containing them will clone if necessary.
Reference-counting some `vec`s and `str`s enables a very important "trick":
Mutating the underlying owned vector or string if only one instance is still alive.

### why rust
Massive piggyback: great stdlib, fast output, good ecosystem, much easier to compile to: native enum support, native pattern matching support, extensive compile-time checks, all that is gold.

You might have heard that compilation can be slow for big projects
but after switching to [the cranelift backend](https://github.com/rust-lang/rustc_codegen_cranelift) I haven't had any complaints (0.7-1.6s, 16k lines).

### why no direct ffi, calling rust from lily
Inspired by elm, effects originate from a single place in your program,
making it easy to: compile to other languages than rust, test in isolation, debug, reorder values without a hidden change in behavior.
If you want to call a specific pure rust function, please ask me to add it to the lily core declarations :3

### how to install packages?
For rust: use `cargo add`.
For lily: just copy paste their code.
For that reason, I recommend lily package authors to follow
```lily
# package-name
# full license
...
# package-name
```
And since licensing is a bit wishy washy like that (and with copy paste in general),
I strongly recommend licensing your lily package under ["unlicense"](https://unlicense.org/) or other public domain/"attribution not required" licenses (e.g. WTFPL or CC0).

## editor setups
feel free to contribute as I only use vscodium

### vscode-like
#### pre-built
1. download https://github.com/lue-bird/lily/blob/main/vscode/lily-0.0.1.vsix
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
    "languageId": "lily",
    "command": "lily",
    "fileExtensions": [
      ".lily"
    ]
  }
]
```

### helix
write to `~/.config/helix/languages.toml`:
```toml
[language-server.lily]
command = "lily lsp"
[[language]]
name = "lily"
scope = "source.lily"
injection-regex = "lily"
file-types = ["lily"]
indent = { tab-width = 4, unit = "    " }
language-servers = [ "lily" ]
auto-format = true
```

## setup for developing
Rebuild the project with
```bash
cargo build
```
Then point your editor to the created `???/target/debug/lily lsp`.

## considering
- (leaning towards yes) allow comments before variant (field name, case?, variant?)
- (leaning towards yes) add `unts-sum`, `decs-sum`, `ints-sum`, `unts-product`, `ints-product`, `decs-product`
- (leaning towards yes) add `vec-walk-backwards-from`, `str-walk-chars-backwards-from`
- (leaning towards no) switch unt and int to 64 bit
- (once a use case is found) add core bitwise and, or, xor, shifts, complement for the integer number types
- (seems not worth the analysis cost but a simpler version maybe is) avoid unnecessary clones by field
- (to make some parts almost infinitely scalable:) for formatting: leave declarations fully outside of "touched ranges" alone; for compilation: if touched only in one declaration and its type ends up the same, only change that declaration's output, (optionally: if type changed, recompile "downstream"); also, when edited range lies exclusively between existing declaration ranges, only compile that one
- in syntax tree, use separate range type for single-line tokens like keywords, symbols, names etc to save on memory consumption
- add `map` (either tree or index map), `set` core types. currently no idea how to implement in few lines in rust. I'd like order functions to be given for each operation
- (maybe in the future) add or pattern `( first | second | third )`
- reimplement [strongly_connected_components](https://docs.rs/strongly-connected-components/latest/strongly_connected_components/) myself

### log of failed optimizations
- switching to mimalloc, ~>25% faster (really nice) at the cost of 25% more memory consumption.
  Might be worth for some people but I'm already worried about our memory footprint!
- `declarations.shrink_to_fit();` saves around 0.6% of memory at the cost of a bit of speed
- upgrading `lto` to `"thin"` to `"fat"` both improve runtime speed by ~13% compared to the default (and reduce binary size) but increase build time by about 30% (default to thin) and 15% (thin to fat).
  As this prolongs installation and prevents people from quickly trying it, the default is kept.
  If this language server get distributed as a binary or people end up using this language server a lot, this `"thin"` might become a reasonable trade-off.
