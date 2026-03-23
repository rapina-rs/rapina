# Handle irregular plurals and uncountable words in codegen singularize/pluralize

## Problem

The `singularize()` and `pluralize()` functions in `rapina-cli/src/commands/codegen.rs` used a pure suffix-stripping approach to convert between singular and plural forms. This worked for regular English plurals (`users` -> `user`, `categories` -> `category`) but failed on anything irregular.

### The bug

`singularize("statuses")` returned `"statu"` instead of `"status"`. The `-ses` suffix rule matched first, stripping it down incorrectly. The existing test acknowledged this with `// naive, acceptable`.

Other broken cases included:
- `singularize("people")` -> `"people"` (should be `"person"`)
- `singularize("indices")` -> `"indic"` (should be `"index"`)
- `singularize("data")` -> `"data"` (should be `"datum"`)
- `pluralize("person")` -> `"persons"` (should be `"people"`)
- `pluralize("sheep")` -> `"sheeps"` (should be `"sheep"`)

These functions are used by `rapina import` and `rapina import-openapi` to derive Rust struct/model names from database table names and API resource paths. Incorrect singularization produces wrong model names in generated code.

## Solution

Added three static lookup tables that are checked **before** the existing suffix rules fall through:

### `IRREGULARS` — 45 (singular, plural) pairs

Covers three categories of irregular forms:

| Category | Examples |
|---|---|
| Common irregulars | `person`/`people`, `child`/`children`, `man`/`men`, `woman`/`women`, `mouse`/`mice`, `goose`/`geese`, `tooth`/`teeth`, `foot`/`feet`, `ox`/`oxen` |
| `-f/-fe` to `-ves` | `knife`/`knives`, `leaf`/`leaves`, `life`/`lives`, `wife`/`wives`, `half`/`halves`, `wolf`/`wolves`, `shelf`/`shelves`, `loaf`/`loaves` |
| Latin/Greek-origin | `index`/`indices`, `datum`/`data`, `medium`/`media`, `matrix`/`matrices`, `analysis`/`analyses`, `thesis`/`theses`, `criterion`/`criteria`, `phenomenon`/`phenomena`, `cactus`/`cacti`, `nucleus`/`nuclei`, `radius`/`radii`, `curriculum`/`curricula`, `formula`/`formulae`, `appendix`/`appendices`, and more |

### `UNCOUNTABLE` — 20 identity words (singular = plural)

Words that do not change form: `series`, `species`, `sheep`, `fish`, `deer`, `moose`, `bison`, `trout`, `salmon`, `shrimp`, `aircraft`, `news`, `info`, `metadata`, `software`, `hardware`, `firmware`, `middleware`, `equipment`, `feedback`.

### `SINGULAR_US` — 11 words ending in `-us`

Words like `status`, `campus`, `virus`, `census`, `corpus`, `opus`, `genus`, `apparatus`, `nexus`, `prospectus`, `consensus`.

These were intentionally separated from `UNCOUNTABLE` because they **do** have distinct plural forms (`status` -> `statuses`), but their trailing `-s` must not be stripped by `singularize`. The separation ensures:
- `singularize("status")` -> `"status"` (not `"statu"`)
- `singularize("statuses")` -> `"status"` (not `"statu"`)
- `pluralize("status")` -> `"statuses"` (not `"status"`)

## How the lookup works

Both functions follow the same priority order:

1. Check `UNCOUNTABLE` — return as-is
2. Check `SINGULAR_US` (singularize only) — return as-is
3. Check `IRREGULARS` — return the mapped form
4. Check `-uses` suffix against `SINGULAR_US` (singularize only) — e.g. `statuses` -> `status`
5. Fall through to existing suffix rules (unchanged)

The `pluralize` function additionally checks if the input is already a known irregular plural (idempotency) and handles `-us` endings with `-es` suffix.

## Files changed

| File | Change |
|---|---|
| `rapina-cli/src/commands/codegen.rs` | Added `IRREGULARS`, `UNCOUNTABLE`, `SINGULAR_US` lookup tables. Updated `singularize()` and `pluralize()` to check lookups before suffix rules. Expanded tests from ~15 to ~200 assertions. Removed `#[cfg(feature = "import")]` gate from `test_singularize`. |
| `rapina-cli/src/commands/import_openapi.rs` | Added 4 assertions to existing `test_singularize`: `statuses`, `people`, `indices`, `series`. |

## Test coverage

Tests now cover five categories for both functions:

1. **Regular plurals** — suffix-based rules (existing, unchanged): `users`/`user`, `categories`/`category`, `addresses`/`address`, `boxes`/`box`, etc.
2. **Irregular plurals** — lookup-driven (new): `people`/`person`, `children`/`child`, `knives`/`knife`, `data`/`datum`, `indices`/`index`, etc.
3. **Uncountable words** — identity (new): `series`, `sheep`, `software`, `metadata`, etc.
4. **Words ending in `-us`** (new): `status`/`statuses`, `campus`/`campuses`, `virus`/`viruses`, etc.
5. **Idempotency** (new): `singularize("person")` -> `"person"`, `pluralize("people")` -> `"people"`, etc.

## Performance considerations

The lookup tables total ~70 entries. Linear scan is fine — these functions are called once per table/resource during code generation, not in hot paths.

## Alternative: using a crate

Three Rust crates solve this problem out of the box:

| Crate | Description |
|---|---|
| [`inflector`](https://crates.io/crates/inflector) | Most popular (~800 stars). Full inflection suite: singular, plural, case conversions. Could also replace `to_pascal_case()`. |
| [`pluralizer`](https://crates.io/crates/pluralizer) | Port of the Node.js `pluralize` library. Very thorough irregular/uncountable coverage. |
| [`inflections`](https://crates.io/crates/inflections) | Lighter alternative with similar API to `inflector`. |

### Trade-offs

**Using a crate:**
- Covers far more edge cases out of the box (hundreds of irregular forms)
- Maintained by the community — new irregulars added over time
- `inflector` could also replace `to_pascal_case()`
- Adds a dependency (and its transitive deps) to the build
- Less control over behavior — may produce surprising results for domain-specific terms
- Overkill for codegen where inputs are database table names (a limited, predictable set)

**The hand-rolled approach (what was implemented):**
- Zero dependencies, ~30 lines of const data
- Covers exactly the cases relevant to database/API resource names
- Easy to extend — just add a tuple to the const array
- Full ownership of behavior, every case is explicit and testable
- Won't handle truly obscure irregulars (but those rarely appear as table names)

For a codegen CLI where the input is database table names, the hand-rolled lookup is the pragmatic choice. A crate like `inflector` would make more sense if building a general-purpose NLP or documentation tool.
