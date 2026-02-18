very small, explicitly boring, purely functional programming language that compiles to rust, inspired by [elm](https://elm-lang.org/).
> ⚠️ Experimental, subject to change, use with caution.

### hello world
```still
greet \:str:name >
    strs-flatten [ "Hello, ", name, "\n" ]
```
Variables don't perform any effects.
The compiled code can however be used from rust to actually do something:
```rust
mod still;
fn main() {
    print!("{}", still::greet(still::Str::Slice("insert your name here")));
}
```
→ complete setup: [`example-hello-world/`](https://github.com/lue-bird/still/tree/main/example-hello-world)

### echo in loop
```still
ansi-clear-screen
    "\u{001B}c"

interface \:opt str:state-or-uninitialized >
    let state
        state-or-uninitialized
        | :opt str:Absent > ""
        | :opt str:Present :str:initialized > initialized
    :io str:Batch
        [ :io str:Standard-out-write
            strs-flatten [ ansi-clear-screen, state, "\nType a sentence to echo: " ]
        , :io str:Standard-in-read-line \:str:line > line
        ]

choice io Future
    | Standard-out-write str
    | Batch vec (io Future)
    | Standard-in-read-line \str > Future
```
→ [`example-echo-in-loop/`](https://github.com/lue-bird/still/tree/main/example-echo-in-loop)

To use, [install rust](https://rust-lang.org/tools/install/) and
```bash
cargo +nightly install --git https://github.com/lue-bird/still
```
Then point your editor to `still lsp`, see also [specific setups](#editor-setups).

## maybe interesting

- each expression and pattern is always concretely typed, if necessary with an explicit annotation. So things like `(++) appendable -> appendable -> appendable`, `0 : number`, `[] : List any` are all not allowed, and e.g. `str-append \:str:l, :str:r > :str:`, `0.0`, `:vec int:[]` are used instead.

  → faster type checking, clear errors, a few less bugs, easy compilation to almost any language

- no blocking compile errors. You can always build, even if your record is still missing a field value, your matching is still inexhaustive, some parens are empty, etc.
  You will still see all the errors, though.

- no features that obfuscate ("shiny, cool features" that ruin languages in my opinion): infix operators, currying, lifetime tracking, traits/type classes/overloading, objects, task/async, hidden mutation, macros & reflection, side effects, modules, hidden context values, exceptions, undefined

## syntax overview
```still
# this is a comment.

# declared variable, type and field names use ascii letters, digits and -
s0me-Name

# any expression can have an explicit :type:
:str:some-variable-name

# string (of type str)
"Yahallo"

# multi-line string (each line starts with `, then raw text until linebreak)
`Ma! I got a thing going here.
`You got lint on your fuzz.
`Ow! That's me!
`Wave to us! "\\\ ` ' \n \r \t \{ \m \u}

# character (of type chr)
'👀'

# signed integer (each of type int, sign is required)
+2012

# unsigned integers (of type unt, no sign)
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
let local-variable-name "Anissa"
strs-flatten [ "Hello, ", local-variable-name, "\n" ]

# an abbreviation for a commonly used type
type point Unity-type-parameter =
    { x Unity-type-parameter, y Unity-type-parameter }

# for expressions, that are either one thing, or some another thing
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
# You will probably prefer `let` for most cases, though.
f x argument
| :f-result:f-result >
g y first-result
| :g-result:g-result >
h z g-result
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

### why no direct ffi, calling rust from still
Inspired by elm, effects originate from a single place in your program,
making it easy to: compile to other languages than rust, test in isolation, debug, reorder values without a hidden change in behavior.
If you want to call a specific pure rust function, please ask me to add it to the still core declarations :3

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
indent = { tab-width = 4, unit = "    " }
language-servers = [ "still" ]
auto-format = true
```

## setup for developing
Rebuild the project with
```bash
cargo build
```
Then point your editor to the created `???/target/debug/still lsp`.

## considering
- provide completions for record field names
- remove record access syntax in favor of destructuring,
  otherwise provide hover info for record field access
- (leaning clear yes) add more core float operations like `sin`, `cos`, `pi`, `ln`
- (leaning towards yes) add core bitwise and, or, xor, shifts, complement for the integer number types
- (leaning towards yes) add `vec-walk-backwards-from`, `str-walk-chrs-backwards-from`
- (leaning towards yes) add `str-attach-unt`, `str-attach-int`, `str-attach-dec`
- (leaning towards yes) rename chr to char
- switch all core numbers to either 32 bit or 64 bit (64 bit would be nice for conversions if there are 32bit variations in the future and also be a reasonable default fur use as posix time or random seed, 32 bit is nice for chr conversion, default memory efficiency)
- (leaning towards yes) allow comments before variant (field name, case?, variant?)
- (to make some parts almost infinitely scalable:) for formatting: leave declarations fully outside of "touched ranges" alone; for compilation: if touched only in one declaration and its type ends up the same, only change that declaration's output, (optionally: if type changed, recompile "downstream"); also, when edited range lies exclusively between existing declaration ranges, only compile that one
- in syntax tree, use separate range type for single-line tokens like keywords, symbols, names etc to save on memory consumption
- (seems not worth the analysis cost but a simpler version maybe is) avoid unnecessary clones by field
- (leaning towards no, sadly) replace non-recursive nominal-ish choice types by structural-ish choice types. Should be fairly easy to implement as `enum Variant0Variant1<Variant0, Variant1>` but still alright for FFI (you always have to type `Variant0Variant1::Variant0` similar to record structs currently _but_ crucially you have the option to use a still-declared type alias like `type Choice<'a> = Variant0Variant1<usize, &'a str>` to write `Choice::Variant0`)
- (currently no idea how to implement in rust, maybe can be done in user land given that it required Hash but I'd like order functions to be given for each operation or similar?) add `map`, `set` core types
- add Range<usize> manually to the Vec::Rc and Str::Rc case (this does trap/"leak" big strings and vecs, JVM seems to have switched away for that reason)
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
- (leaning towards no) allow concrete bounded variables in some type aliases and choice types instead of &dyn
- (maybe in the future) add or pattern `( first | second | third )`
- reimplement [strongly_connected_components](https://docs.rs/strongly-connected-components/latest/strongly_connected_components/) myself

### log of failed optimizations
- switching to mimalloc, ~>25% faster (really nice) at the cost of 25% more memory consumption.
  Might be worth for some people but I'm already worried about our memory footprint!
- `declarations.shrink_to_fit();` saves around 0.6% of memory at the cost of a bit of speed
- upgrading `lto` to `"thin"` to `"fat"` both improve runtime speed by ~13% compared to the default (and reduce binary size) but increase build time by about 30% (default to thin) and 15% (thin to fat).
  As this prolongs installation and prevents people from quickly trying it, the default is kept.
  If this language server get distributed as a binary or people end up using this language server a lot, this `"thin"` might become a reasonable trade-off.
