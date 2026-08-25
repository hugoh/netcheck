use std::fs;
use std::path::Path;
use std::process::ExitCode;

use comrak::{Options, markdown_to_html};

const README_PATH: &str = "README.md";
const SITE_DIR: &str = "site";
const SITE_NAME: &str = "netcheck";
const REPO_URL: &str = "https://github.com/hugoh/netcheck";
const SITE_URL: &str = "https://hugoh.github.io/netcheck/";

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let readme = fs::read_to_string(README_PATH)?;
    let description = extract_tagline(&readme);
    let body = render_body(&readme);

    fs::create_dir_all(SITE_DIR)?;
    fs::write(
        Path::new(SITE_DIR).join("index.html"),
        render_page(&description, &body),
    )?;

    Ok(())
}

/// Joins the first prose paragraph after the README's H1 into one line, for
/// use as the page's `<title>`/`<meta description>`/`og:description`.
fn extract_tagline(readme: &str) -> String {
    let mut lines = readme.lines();

    // Consume everything up to and including the H1 heading line.
    for line in lines.by_ref() {
        if !line.trim().is_empty() {
            break;
        }
    }

    let mut paragraph = Vec::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line.is_empty() {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        paragraph.push(line);
    }

    paragraph.join(" ")
}

/// Renders the README to HTML, rewriting its GitHub-relative screenshot
/// paths to match where the generated site actually serves them from.
fn render_body(readme: &str) -> String {
    let readme = readme
        .replace("assets/screenshots/", "screenshots/")
        .replace("assets/icon-1024.png", "favicon.png");

    let mut options = Options::default();
    options.extension.table = true;
    options.extension.alerts = true;
    options.extension.header_id_prefix = Some(String::new());

    let html = markdown_to_html(&readme, &options);

    html.replace(
        r#"<img src="screenshots/"#,
        r#"<img class="screenshot" src="screenshots/"#,
    )
}

fn render_page(description: &str, body: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{SITE_NAME} — {description}</title>
<meta name="description" content="{description}">
<link rel="canonical" href="{SITE_URL}">
<link rel="icon" type="image/png" href="favicon.png">
<meta property="og:type" content="website">
<meta property="og:title" content="{SITE_NAME}">
<meta property="og:description" content="{description}">
<meta property="og:url" content="{SITE_URL}">
<link rel="stylesheet"
  href="https://cdn.jsdelivr.net/npm/@picocss/pico@2.1.1/css/pico.classless.min.css"
  integrity="sha384-NZhm4G1I7BpEGdjDKnzEfy3d78xvy7ECKUwwnKTYi036z42IyF056PbHfpQLIYgL" crossorigin="anonymous">
<link rel="stylesheet" href="style.css">
</head>
<body>
<nav class="topnav">
<a class="brand" href="#"><img src="favicon.png" alt="" width="24" height="24">{SITE_NAME}</a>
<a href="#install">Install</a>
<a href="#the-two-flavors">The two flavors</a>
<a href="#screenshots">Screenshots</a>
<a href="#usage">Usage</a>
<a href="{REPO_URL}">GitHub</a>
</nav>
<main>
{body}
</main>
<footer>
<p>Source, issues, and releases on <a href="{REPO_URL}">GitHub</a>.</p>
</footer>
</body>
</html>
"##
    )
}
