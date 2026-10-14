use clap::{Parser, ValueEnum};
use std::io::{self, BufRead, Write};
use place_cards::fonts::{FontSpec, Fonts};
use place_cards::{Guest, parse_menu, pdf, read_csv, sort_by_table};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum SortBy {
    Table,
}

/// Generate printable fold-over table place cards as an A4 PDF.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// CSV file with columns name,table,menu. Omit for interactive entry.
    csv: Option<PathBuf>,
    /// Output PDF path.
    #[arg(short, long, default_value = "cards.pdf")]
    output: PathBuf,
    /// Small event title printed on each card, e.g. "Smith Wedding".
    #[arg(long)]
    title: Option<String>,
    /// Font for the event title: a built-in name (e.g. Times-Italic), "EB Garamond" / "EB Garamond Italic" / "EB Garamond Bold" / "EB Garamond Bold Italic", or a path to a .ttf/.otf. Default Helvetica.
    #[arg(long)]
    title_font: Option<String>,
    /// Font for the guest name. Default Helvetica-Bold.
    #[arg(long)]
    name_font: Option<String>,
    /// Font for the "Table N" line. Default Helvetica.
    #[arg(long)]
    table_font: Option<String>,
    /// Font for the menu choices. Default Helvetica.
    #[arg(long)]
    menu_font: Option<String>,
    /// Also write a seating plan & menu PDF (for back of house) to this path.
    #[arg(long, value_name = "PDF")]
    plan: Option<PathBuf>,
    /// Write only the seating plan (needs --plan); the cards PDF is not written.
    #[arg(long, requires = "plan")]
    plan_only: bool,
    /// Force landscape for the seating plan (otherwise chosen automatically).
    #[arg(long)]
    plan_landscape: bool,
    /// Do not draw the small allergy marker on cards of guests with dietary entries.
    #[arg(long)]
    no_allergy_marker: bool,
    /// Sort guests before layout.
    #[arg(long, value_enum)]
    sort: Option<SortBy>,
}

fn prompt(label: &str) -> io::Result<Option<String>> {
    print!("{label}");
    io::stdout().flush()?;
    let mut s = String::new();
    if io::stdin().lock().read_line(&mut s)? == 0 {
        return Ok(None); // EOF
    }
    Ok(Some(s.trim().to_string()))
}

fn interactive() -> io::Result<Vec<Guest>> {
    let mut guests = Vec::new();
    println!("Enter guests one at a time. Leave Name blank to finish.");
    loop {
        let name = match prompt("Name: ")? {
            Some(n) if !n.is_empty() => n,
            _ => break,
        };
        let table = loop {
            match prompt("Table No.: ")? {
                Some(t) if !t.is_empty() => break t,
                Some(_) => println!("Table number is required."),
                None => return Ok(guests),
            }
        };
        let menu = parse_menu(&prompt("Menu choices (optional, ';' separated, blank to skip): ")?.unwrap_or_default());
        guests.push(Guest { name, table, menu, dietary: Vec::new() });
    }
    Ok(guests)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut guests = match &cli.csv {
        Some(p) => read_csv(p)?,
        None => interactive()?,
    };
    if guests.is_empty() {
        return Err("no guests supplied; nothing to write".into());
    }
    // The plan uses the list order (grouped by table), not the card sort.
    let plan_guests = guests.clone();
    if let Some(SortBy::Table) = cli.sort {
        // Numeric tables first in numeric order, then others alphabetically.
        sort_by_table(&mut guests);
    }
    if let Some(plan) = &cli.plan {
        let info = place_cards::plan::render_plan_ex(&plan_guests, cli.title.as_deref(), cli.plan_landscape, !cli.no_allergy_marker, plan)?;
        println!(
            "Wrote seating plan ({} page(s), {}) to {}",
            info.pages,
            if info.landscape { "landscape" } else { "portrait" },
            plan.display()
        );
        if cli.plan_only {
            return Ok(());
        }
    }
    let mut fonts = Fonts::default();
    for (opt, slot) in [
        (&cli.title_font, &mut fonts.title),
        (&cli.name_font, &mut fonts.name),
        (&cli.table_font, &mut fonts.table),
        (&cli.menu_font, &mut fonts.menu),
    ] {
        if let Some(s) = opt {
            *slot = FontSpec::parse(s)?;
        }
    }
    let (pages, warnings) = pdf::render_opts(
        &guests,
        cli.title.as_deref(),
        &fonts,
        &pdf::CardOptions { allergy_marker: !cli.no_allergy_marker },
        &cli.output,
    )?;
    for w in warnings {
        eprintln!("warning: {w}");
    }
    println!(
        "Wrote {} cards on {} page(s) to {}",
        guests.len(),
        pages,
        cli.output.display()
    );
    Ok(())
}
