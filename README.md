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

## Courses and menu entries

An event has 1 to 6 **courses** (default 3). Default names by count: 1 Main; 2 Main, Dessert;
3 Starter, Main, Dessert; 4 Starter, Soup, Main, Dessert; 5 Starter, Soup, Fish, Main,
Dessert; 6 Canapés, Starter, Soup, Fish, Main, Dessert. In the GUI, the **Courses** row near
the event title has a count selector and editable names (remembered between launches). Changing
the count resets the names to that count's defaults unless you have edited them; once edited they
are kept (courses are added/removed at the end). Dishes are re-keyed by course name, so nothing
moves to the wrong course, and the dishes of a removed course are kept as "Other" entries.

Each guest has **one field per course**: just type "Soup", "Beef", "Tart" (leave a course blank
to skip it). The small **v** button next to a field lists the dishes already used for that course,
so spelling stays consistent. Cards still print "Starter: Soup" etc.

### Menu column in CSV

The `menu` column is a `;`-separated list. Entries can be labelled or positional:

    name,table,menu
    Ann,1,Starter: Soup; Main: Beef; Dessert: Tart     <- labelled
    Bob,1,Soup; Beef; Tart                             <- positional: Starter, Main, Dessert
    Cy,2,; Beef; Tart                                  <- empty slot skips the Starter
    Di,2,Soup;;Tart                                    <- skips the Main
    Ed,3,Soup; Chicken                                 <- trailing course omitted
    Flo,3,Main: Lamb; Soup; Tart                       <- labelled ones claim their course, the rest fill the others in order

Rules: labelled entries ("Main: Beef") go to that course (case-insensitive); an unknown label adds a
new course after the known ones. Unlabelled entries fill the event's courses by position (mixed rows:
the courses not claimed by a label, in order). Extra unlabelled entries are kept under "Other" and the
import warns, naming the guest.

When importing, the courses are **inferred**: the longest menu row with any unlabelled entries sets
the count (that count's default names, at most 6), and any course labels in the file are added to
those. Known names keep the standard order Canapés, Starter, Soup, Fish, Main, Dessert. A fully
labelled file uses just its labels. The GUI status line says e.g. "Detected 3 courses: Starter,
Main, Dessert"; edit the names or count afterwards. Your own courses are kept when you have edited
them or are appending to an existing list (only new labels are added). Save CSV writes labelled
entries ("Starter: Soup; Main: Beef") so files are unambiguous. The CLI prints the detected courses
and accepts `--courses 3` or `--courses "Starter,Main,Dessert"` to override. The seating plan's course
columns follow the event's course order.

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
column headers repeated. Text is always Helvetica and nothing is larger than 12 pt
(page title 12 pt bold, section headings 11, table sub-headings 10, body and column
headers 9, footer and key 8). Long names and dishes wrap up to 3 lines (dietary text up to 6) and then
end in an ellipsis. Requirements are written as "Allergens: ... · Diet: ... · Other: ..." (empty parts omitted); the page-1 totals list per-allergen counts first, then preferences, then Other. Guests with a dietary entry are shown in bold with a heavier grey band, a "!" marker and a black bar in the margin (clear in black-and-white), and each table gets an extra "! Dietary: ..." band under its summary. There is no seat numbering (no seat data).

### Two sheets, each scaled to one page

The plan is two fixed sheets, each starting on a new page and **scaled to fit one page**:

1. **Kitchen**: the dietary requirements & allergies list, kitchen totals (standard + dietary = total),
   dietary / allergy portions, the dishes-by-table matrix and the allergy key line.
2. **Tables**: every table's guests with their course dishes and the Dietary / allergies column next to
   the name (highlighted with "!", bold, a grey band and a black bar), plus the per-table runner summary
   of **standard** portions ("No standard portions" if everyone there has a requirement). There is no
   separate per-table dietary band: the dietary text is in each guest's row and is always wrapped in full
   (never cut with an ellipsis).

Each sheet's text starts at 9 pt and is shrunk in half-point steps (headings scale with it and never
exceed 12 pt) to a **6 pt floor** until the sheet fits one page. Each sheet picks portrait or landscape
(whichever allows the larger font; `--plan-landscape` / the GUI checkbox force landscape for both) and
uses 1-3 (Kitchen) or 1-4 (Tables) newspaper columns where that helps; a table is not split across columns
unless it is taller than a column, except at the 6 pt floor when splitting saves a page (and multi-column
layouts are used only when no word would have to be cut). If a sheet still does not fit at 6 pt it
continues onto more pages ("Table N (cont.)", repeated headers) and a warning is printed by the CLI
("Tables sheet needs 2 pages even at 6 pt") and shown in the GUI status line. The page header names the
sheet ("... Seating plan & menu - Kitchen"); the date and "Page X of Y" stay. The dish-by-table matrix is
dropped (with a note) only if even a 6 pt matrix would not fit the page width.

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
