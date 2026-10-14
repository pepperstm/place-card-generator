<p align="center"><img src="assets/icon/AppIcon-1024.png" width="128" alt="Place Cards icon"></p>

# place-cards

Generate printable fold-over (tent) table place cards as an A4 PDF.

## Build

    cargo build --release

## Usage

    # From a CSV
    place-cards guests.csv -o cards.pdf [--title "Smith Wedding"] [--sort table]

    # Interactive: prompts for Name, Table No., Menu choices until a blank name
    place-cards -o cards.pdf

Options: `-o/--output` (default `cards.pdf`), `--title` (small event title above
each name), `--sort table` (by table number, then name; numeric tables first).

## CSV format

Header row `name,table,menu,dietary`. `name` and `table` are required; `menu` and
`dietary` may be empty. Columns are matched by header name (any case, any order), and
`dietary` may also be called `allergies` or `dietary/allergies`; files with only
`name,table,menu` (or no `menu` column) still load. Separate several choices with `;` and quote the field if it has commas:

    name,table,menu,dietary
    Alice Johnson,1,Starter: Soup; Main: Beef; Dessert: Tart,
    Bob Smith,1,Starter: Salad; Main: Salmon,Gluten; Crustaceans; Molluscs
    Charlotte Davies,2,,Peanuts; Tree nuts
    Zoë Müller,4,Main: Beef,Milk; no coriander

## Layout

- A4 portrait, 6 cards per page (2 x 3); extra pages are added automatically.
- Most printers cannot print the outer ~4 mm, so the grid is inset 5 mm on all
  sides: each cell is 100 x 95.67 mm (not 105 x 99) and folds into a
  100 x 47.83 mm face. Text keeps >= 5 mm padding from cell edges.
- Bottom half of each cell is upright; the top half is the same content rotated
  180 degrees, so it reads from both sides once folded.
- Dashed grey lines are cut lines; the lighter dotted line is the fold.
- Long names shrink to fit (minimum 8 pt).
- Uses built-in Helvetica; accented Latin-1 characters work, other scripts
  (CJK, emoji) are not supported.

## Printing tips

- Print at 100% / "Actual size", NOT "Fit to page", or the cards will be scaled.
- Use card stock around 200-250 gsm; score along the fold line before folding.
- Cut along the dashed lines.

## Dietary requirements / allergies

Each guest has an optional dietary list, stored as `;`-separated text in the CSV
`dietary` column and edited in the GUI with tick boxes (**Edit...** in the guest
table, or **Dietary / allergies...** in the add-guest row):

- **Allergens (the UK/EU 14):** Celery, Gluten (cereals containing gluten),
  Crustaceans, Eggs, Fish, Lupin, Milk, Molluscs, Mustard, Tree nuts, Peanuts,
  Sesame, Soya, Sulphites. Hover a box for the full legal name.
- **Dietary preferences:** Vegetarian, Vegan, Pescatarian, Halal, Kosher.
- **Other:** a free-text box (several entries separated by `;`; typing ticks the box).

Entries are saved in canonical form: allergens in the order above, then
preferences, then Other. On CSV import, entries that clearly match a box are
ticked ("Milk allergy", "Peanut allergy", "Egg allergy", "Gluten free", "Coeliac",
"Soy", "Sulfites", "Sulphur dioxide", "Sesame seeds", "Dairy", "Dairy free", the full
legal names, ...). **Ambiguous entries are never guessed: "Nut allergy" (peanuts or
tree nuts?) and "Shellfish allergy" (crustaceans or molluscs?) stay in Other, unchanged,
so you can re-tick them properly.** It is shown in the seating plan only (as
"Allergens: ... · Diet: ... · Other: ..."), never on place cards, apart from the
optional marker dot below.

## Seating plan & menu (back of house)

A separate A4 PDF for food running, from the same guest list.

    # cards AND plan
    place-cards guests.csv -o cards.pdf --plan plan.pdf --title "Smith Wedding"
    # plan only (no cards PDF)
    place-cards guests.csv --plan plan.pdf --plan-only
    # force landscape
    place-cards guests.csv --plan plan.pdf --plan-only --plan-landscape

In the GUI use **Seating plan PDF...** (default name `seating-plan.pdf`, with a
"landscape" checkbox); it applies the same name/table validation as the cards.

Menu choices are split on their first `:` into course and dish ("Starter: Soup"),
matched case-insensitively (first-seen spelling kept). Choices without a colon go
in an "Other" column. Contents: a **Dietary requirements & allergies** section first on page 1 (every flagged guest as Table / Guest / Requirements with a count per requirement, merged case-insensitively; if nobody has any it says "No dietary requirements recorded."), then **Kitchen totals**, which separates **Standard portions** (guests with no dietary entries; each dish shows Standard + Dietary = Total so the numbers reconcile, and a dish whose diners all have requirements still appears with 0 standard) from **Dietary / allergy portions** (a highlighted table by course, dish and the guest's full requirement combination, e.g. "Gluten free + Shellfish allergy", with count and guests with tables; guests with requirements but no menu choices appear under "No menu choices recorded"). Every dietary entry, including Vegetarian, is treated as needing a special portion. The **Dishes by table (all portions, incl. dietary)** matrix marks cells containing a dietary portion with "*"; the per-table summary counts standard portions and lists each modified plate in the dietary band. Then (each dish counted, total
guests, guests with no menu choices), a **Dishes by table** matrix (up to 12
tables), then one section per table (numeric tables first, then alphabetical)
with a row per guest in list order, a "-" for missing courses, a "Dietary / allergies" column, and a runner
summary such as `Main: Beef x2, Salmon x1 · Dessert: Tart x3`. Every page has the
event title, "Seating plan & menu", the date and "Page X of Y". A table is kept
on one page when it fits, otherwise it continues as "Table N (cont.)" with the
column headers repeated. Text is always Helvetica (body 11 pt, headers 14 pt+);
long names and dishes wrap up to 3 lines and then end in an ellipsis. Portrait is
used unless there are more than 3 course columns, more than 8 tables, or
landscape is forced. Requirements are written as "Allergens: ... · Diet: ... · Other: ..." (empty parts omitted); the page-1 totals list per-allergen counts first, then preferences, then Other. Guests with a dietary entry are shown in bold with a heavier grey band, a "!" marker and a black bar in the margin (clear in black-and-white), and each table gets an extra "! Dietary: ..." band under its summary. There is no seat numbering (no seat data).

## Fonts

Each text element (title, name, table line, menu) has its own font. Defaults:
title Helvetica, name Helvetica-Bold, table Helvetica, menu Helvetica.

    place-cards guests.csv -o cards.pdf \
      --name-font /System/Library/Fonts/Supplemental/Georgia.ttf \
      --menu-font Times-Italic --table-font Courier --title-font Times-Roman

Each `--title-font/--name-font/--table-font/--menu-font` takes a built-in name
(Helvetica, Helvetica-Bold, -Oblique, -BoldOblique, Times-Roman, Times-Bold,
Times-Italic, Times-BoldItalic, Courier, Courier-Bold, -Oblique, -BoldOblique;
case-insensitive) or a path to a TrueType `.ttf` (or `.otf` with TrueType
outlines, or CFF-based OpenType such as Adobe Garamond Premier Pro). Fonts are embedded once per PDF. If a font file cannot be
embedded the default font is used and a warning is printed. Centring and
shrink-to-fit use real metrics (AFM tables for built-ins, advance widths for
TrueType). Text is limited to what the font has glyphs for.

### Bundled font: EB Garamond

EB Garamond (Regular, Italic, Bold, Bold Italic) is compiled into the program,
so it works on any Mac without installing anything. Use the names
`"EB Garamond"`, `"EB Garamond Italic"`, `"EB Garamond Bold"` and
`"EB Garamond Bold Italic"` (case-insensitive) on the command line; they also
appear in the GUI font menus. It is (c) The EB Garamond Project Authors and
licensed under the SIL Open Font License 1.1; see `assets/fonts/OFL.txt`.

### CFF-based .otf fonts

OpenType fonts with CFF outlines (files starting with "OTTO", e.g. Garamond
Premier Pro, Garamond 3 LT Std) are supported: they appear in the GUI list and can
be passed by path on the CLI. They are embedded whole (not subset), so each
distinct CFF font adds roughly its file size (about 100-250 KB) to the PDF. EB
Garamond and other TrueType fonts are also embedded whole (about 0.6 MB each).
`.ttc`/`.otc` collections are still skipped.

## GUI

A native window version (egui/eframe) that uses the same PDF code.

    # Run from source
    cargo run --release --bin place-cards-gui

    # Build a double-clickable app at dist/Place Cards.app
    ./bundle.sh
    open "dist/Place Cards.app"

The **Fonts** section has a searchable dropdown per element listing the built-in
fonts and the TrueType fonts found in /System/Library/Fonts, /Library/Fonts and
~/Library/Fonts (scanned in the background at start-up; .ttc collections and
CFF-based .otf are skipped). Choices are remembered between launches.

Type an event title, add guests (press Enter in Name, Table or Menu to add),
edit rows in place, and use **Import CSV...** (tick "replace list" to replace
instead of append), **Save CSV...** and **Generate PDF...** (opens the PDF when
done). Rows missing a name or table are highlighted and block PDF generation.
The status bar shows e.g. "8 guests -> 2 pages". `cargo run` with no `--bin`
still runs the CLI. The app is ad-hoc signed only; if macOS blocks it,
right-click > Open the first time. It is not installed anywhere.

### Live preview

The right-hand panel shows the selected guest's full unfolded card (upright
bottom half, 180-degree top half, cut outline and fold line) and updates as you
type or change fonts. Click a row number (or edit any field in a row) to choose
the guest; with no guests it previews the add-guest fields, then a sample. The
**Page** toggle shows the whole A4 sheet that guest lands on (respecting "Sort
by table"). The preview is drawn from the same layout function as the PDF, using
the same fonts: built-ins map to the macOS Helvetica/Times/Courier faces, and
TrueType choices are loaded from their files. If a font cannot be previewed a
note appears and an approximate font is used. Setting `PLACE_CARDS_DEMO=1` when
launching the binary pre-loads the sample guests (debug aid, nothing is saved).

## App icon

`assets/icon/make_icon.swift` draws the icon with CoreGraphics; `assets/icon/make_icon.sh`
regenerates `AppIcon-1024.png`, `icon-256.png` (used for the window icon) and
`AppIcon.icns` (built from an iconset under `target/`). `./bundle.sh` copies the
`.icns` into `dist/Place Cards.app`.

## Licence

The code is released under the MIT Licence; see `LICENSE`. The bundled EB Garamond fonts in `assets/fonts/` are under the SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
