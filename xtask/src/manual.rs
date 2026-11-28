//! `cargo xtask manual [--lang ja] [--pdf]` (M15.1): checks the user manual of a language (docs/manual/<lang>/) and
//! puts its chapters together into one Markdown file, the source of a PDF.
//!
//! The check: every link to a chapter (`NN-name.md`, `NN-name.md#heading`, `#heading`) names a chapter and a heading
//! that exist, and every picture (`../images/<lang>/...`) is there, in the chapter's own language. `cargo xtask ci`
//! runs it for every language.
//!
//! The book (`target/manual/<lang>/disccraft-manual.md`): the chapters in their order, a page break before each, the
//! navigation lines (`[目次](README.md) …`) left out, every heading given its anchor as an explicit id (`{#...}`, as
//! GitHub makes them), the links between chapters made links inside the book, links to other documents made plain
//! text, and the pictures copied beside it. With `--pdf` (M15.4), the book is also made one HTML page (a title page,
//! the contents, a chapter a page, the pictures with their captions, page numbers) and a browser without a window (Edge,
//! Chrome or Chromium; `DISCCRAFT_BROWSER`) prints it as `disccraft-manual.pdf`: nothing else to install, and the
//! Japanese is set in the system's fonts. The contents are in two columns with the page of every chapter and section:
//! the page is printed, the pages its headings landed on are read from the PDF's named destinations, and it is
//! printed again with them (M15.6). Each chapter starts a page; one whose last page would hold only a few lines is set
//! a little tighter where that brings them back (its end's height is read from the PDF the same way; M15.8).
//!
//! `cargo xtask manual --release <tag>` (M15.17) makes the PDF of every language with the release's version on the
//! title page, for the GitHub release: `target/release-assets/disccraft-<version>-manual-<lang>.pdf`, refused when it
//! holds a path of this computer.

use crate::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A chapter: its file name, its text.
struct Chapter {
    name: String,
    text: String,
}

/// The anchor GitHub gives a heading: lower case, punctuation left out (letters of any script, digits, `_` and `-`
/// stay), spaces made hyphens.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The headings of a chapter (outside code blocks): (level, text).
fn headings(text: &str) -> Vec<(usize, String)> {
    let mut fenced = false;
    let mut found = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && line[level..].starts_with(' ') {
            found.push((level, line[level..].trim().to_owned()));
        }
    }
    found
}

/// Whether `name` is a chapter's file name (`NN-name.md`).
fn is_chapter(name: &str) -> bool {
    let bytes = name.as_bytes();
    name.ends_with(".md")
        && bytes.len() > 6
        && bytes.first().is_some_and(u8::is_ascii_digit)
        && bytes.get(1).is_some_and(u8::is_ascii_digit)
        && bytes.get(2) == Some(&b'-')
}

/// The chapters of the manual in `folder`, in order.
fn chapters(folder: &Path) -> Result<Vec<Chapter>> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_chapter(name))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = folder.join(&name);
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Ok(Chapter { name, text })
        })
        .collect()
}

/// The Markdown links of `text` outside code (`](target)`), with the line each is on.
fn links(text: &str) -> Vec<(usize, String, bool)> {
    let mut fenced = false;
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let picture = rest[..at]
                .rfind("![")
                .is_some_and(|open| !rest[open..at].contains("]("));
            let after = &rest[at + 2..];
            let Some(end) = after.find(')') else {
                break;
            };
            found.push((number + 1, after[..end].to_owned(), picture));
            rest = &after[end..];
        }
    }
    found
}

/// The number a link's words start with, when they name a chapter or a section (`5.1`, `5.1 読み込み方`, `14 章`,
/// `Chapter 14`, `3. はじめての DVD を作る`): the chapter's, and the section's within it.
fn link_number(words: &str) -> Option<(u32, Option<u32>)> {
    let digits = |text: &str| text.chars().take_while(char::is_ascii_digit).count();
    if let Some(number) = words.strip_prefix("Chapter ") {
        let chapter_len = digits(number);
        let rest = number.get(chapter_len..)?;
        return (chapter_len > 0 && (rest.is_empty() || rest.starts_with(' ')))
            .then(|| Some((number.get(..chapter_len)?.parse().ok()?, None)))
            .flatten();
    }
    let chapter_len = digits(words);
    let chapter = words.get(..chapter_len)?.parse().ok()?;
    let rest = words.get(chapter_len..)?;
    if rest.is_empty() || rest.starts_with(" 章") {
        return Some((chapter, None));
    }
    let after_dot = rest.strip_prefix('.')?;
    if after_dot.is_empty() || after_dot.starts_with(' ') {
        return Some((chapter, None));
    }
    let section_len = digits(after_dot);
    let section = after_dot.get(..section_len)?.parse().ok()?;
    let tail = after_dot.get(section_len..)?;
    (tail.is_empty() || tail.starts_with(' ')).then_some((chapter, Some(section)))
}

/// The links of the chapter `name` (its text `text`) whose words give a chapter's or a section's number but that go to
/// another one: a number left behind when the chapters or sections were numbered again. (line, what is wrong)
fn misnumbered(name: &str, text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut fenced = false;
    for (number, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let head = &rest[..at];
            let after = &rest[at + 2..];
            let Some(end) = after.find(')') else {
                break;
            };
            let target = &after[..end];
            rest = &after[end..];
            let Some(open) = head.rfind('[') else {
                continue;
            };
            if head[..open].ends_with('!') {
                continue;
            }
            let Some((chapter, section)) = link_number(&head[open + 1..]) else {
                continue;
            };
            let (file, heading) = target.split_once('#').unwrap_or((target, ""));
            let file = if file.is_empty() { name } else { file };
            if !is_chapter(file) {
                continue;
            }
            let file_chapter: Option<u32> = file.get(..2).and_then(|digits| digits.parse().ok());
            let wrong_chapter = file_chapter != Some(chapter);
            let wrong_section = section.is_some_and(|section| {
                !heading.is_empty() && !heading.starts_with(&format!("{chapter}{section}-"))
            });
            if wrong_chapter || wrong_section {
                found.push((
                    number + 1,
                    format!(
                        "[{}]({target}) does not go to that number",
                        &head[open + 1..]
                    ),
                ));
            }
        }
    }
    found
}

/// The problems of the manual in `folder` (language `lang`): links to missing chapters or headings, missing pictures,
/// pictures of another language.
fn check(root: &Path, lang: &str) -> Result<Vec<String>> {
    let folder = root.join("docs").join("manual").join(lang);
    let mut files: Vec<Chapter> = chapters(&folder)?;
    let readme = folder.join("README.md");
    if let Ok(text) = std::fs::read_to_string(&readme) {
        files.push(Chapter {
            name: "README.md".to_owned(),
            text,
        });
    }
    let anchors: BTreeMap<&str, BTreeSet<String>> = files
        .iter()
        .map(|chapter| {
            let found = headings(&chapter.text)
                .into_iter()
                .map(|(_, text)| anchor(&text))
                .collect();
            (chapter.name.as_str(), found)
        })
        .collect();
    let mut problems = Vec::new();
    for chapter in &files {
        for (line, problem) in misnumbered(&chapter.name, &chapter.text) {
            problems.push(format!(
                "docs/manual/{lang}/{}:{line}: {problem}",
                chapter.name
            ));
        }
        for (line, target, picture) in links(&chapter.text) {
            let place = format!("docs/manual/{lang}/{}:{line}", chapter.name);
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
            {
                continue;
            }
            if picture {
                let wanted = format!("../images/{lang}/");
                if !target.starts_with(&wanted) {
                    problems.push(format!(
                        "{place}: the picture {target} is not under {wanted}"
                    ));
                }
                if !folder.join(&target).is_file() {
                    problems.push(format!("{place}: no picture {target}"));
                }
                continue;
            }
            let (file, heading) = match target.split_once('#') {
                Some((file, heading)) => (file, Some(heading)),
                None => (target.as_str(), None),
            };
            let file = if file.is_empty() {
                chapter.name.as_str()
            } else {
                file
            };
            if let Some(known) = anchors.get(file) {
                if let Some(heading) = heading
                    && !known.contains(heading)
                {
                    problems.push(format!("{place}: {file} has no heading #{heading}"));
                }
            } else if !folder.join(file).exists() {
                problems.push(format!("{place}: no file {file}"));
            }
        }
    }
    Ok(problems)
}

/// `cargo xtask manual`: checks the manuals and, for `--lang`, writes the book; `--release <tag>` makes the PDF of every
/// language for that release.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    if let Some(tag) = args
        .iter()
        .position(|arg| arg == "--release")
        .and_then(|at| args.get(at + 1))
    {
        return release(root, tag);
    }
    let lang = args
        .iter()
        .position(|arg| arg == "--lang")
        .and_then(|at| args.get(at + 1))
        .map_or("ja", String::as_str);
    if lang.is_empty() || !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("{lang}: not a language code"));
    }
    let problems = check(root, lang)?;
    if !problems.is_empty() {
        return Err(format!("the {lang} manual:\n{}", problems.join("\n")));
    }
    let out = root.join("target").join("manual").join(lang);
    let book = write_book(root, lang, &out)?;
    println!("manual ({lang}): {}", book.display());
    if args.iter().any(|arg| arg == "--pdf") {
        pdf(&book, lang, &book_title(root, lang), &version(root))?;
    }
    Ok(())
}

/// The version of a release tag (`v0.1.0-alpha.15` → `0.1.0-alpha.15`), when it is one.
fn release_version(tag: &str) -> Option<&str> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    (version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
    .then_some(version)
}

/// The languages of the manual: the folders under docs/manual but the pictures', in order.
fn languages(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root.join("docs").join("manual"))
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name != "images")
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The paths of this computer that a published file must not hold: the repository's and the home folder's, with
/// either kind of slash, and links to local files.
fn private_paths(root: &Path) -> Vec<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let mut found = vec!["file:/".to_owned()];
    for path in [Some(root.to_path_buf()), home].into_iter().flatten() {
        let text = path.to_string_lossy();
        let text = text.trim_start_matches(r"\\?\");
        if text.len() > 3 {
            found.push(text.replace('/', "\\"));
            found.push(text.replace('\\', "/"));
        }
    }
    found
}

/// `--release <tag>`: the PDF of every language with the release's version on its title page, checked for paths of
/// this computer, copied for the release as `target/release-assets/disccraft-<version>-manual-<lang>.pdf`.
fn release(root: &Path, tag: &str) -> Result<()> {
    let version =
        release_version(tag).ok_or(format!("{tag}: not a release tag (v0.1.0-alpha.15)"))?;
    let assets = root.join("target").join("release-assets");
    std::fs::create_dir_all(&assets).map_err(|error| format!("{}: {error}", assets.display()))?;
    let private = private_paths(root);
    for lang in languages(root) {
        let problems = check(root, &lang)?;
        if !problems.is_empty() {
            return Err(format!("the {lang} manual:\n{}", problems.join("\n")));
        }
        let book = write_book(root, &lang, &root.join("target").join("manual").join(&lang))?;
        pdf(&book, &lang, &book_title(root, &lang), version)?;
        let printed = book.with_extension("pdf");
        let bytes =
            std::fs::read(&printed).map_err(|error| format!("{}: {error}", printed.display()))?;
        if let Some(found) = private
            .iter()
            .find(|path| find(&bytes, path.as_bytes()).is_some())
        {
            return Err(format!(
                "{} holds \"{found}\": not to be published",
                printed.display()
            ));
        }
        let target = assets.join(format!("disccraft-{version}-manual-{lang}.pdf"));
        std::fs::copy(&printed, &target)
            .map_err(|error| format!("{}: {error}", target.display()))?;
        println!("manual ({lang}): {}", target.display());
    }
    Ok(())
}

/// The manuals' check for `cargo xtask ci`: every language under docs/manual.
pub(crate) fn check_all(root: &Path) -> Result<()> {
    let manuals = root.join("docs").join("manual");
    let Ok(entries) = std::fs::read_dir(&manuals) else {
        return Ok(());
    };
    let mut problems = Vec::new();
    let mut checked = 0;
    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name == "images" || !entry.path().is_dir() {
            continue;
        }
        problems.extend(check(root, &name)?);
        checked += 1;
    }
    if problems.is_empty() {
        println!("manual: {checked} languages OK");
        Ok(())
    } else {
        Err(format!("the manual:\n{}", problems.join("\n")))
    }
}

/// The manual's title: the first heading of its contents page.
fn book_title(root: &Path, lang: &str) -> String {
    std::fs::read_to_string(
        root.join("docs")
            .join("manual")
            .join(lang)
            .join("README.md"),
    )
    .ok()
    .and_then(|text| {
        headings(&text)
            .into_iter()
            .find(|(level, _)| *level == 1)
            .map(|(_, text)| text)
    })
    .unwrap_or_else(|| "DiscCraft".to_owned())
}

/// The version of the workspace (`version` of `[workspace.package]`), for the book's title page.
fn version(root: &Path) -> String {
    std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| {
            text.lines()
                .skip_while(|line| line.trim() != "[workspace.package]")
                .find_map(|line| {
                    line.trim()
                        .strip_prefix("version = ")
                        .map(|v| v.trim_matches('"').to_owned())
                })
        })
        .unwrap_or_default()
}

/// Copies the folder `from` into `to`, recursively.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
        }
    }
    Ok(())
}

/// The chapters of `lang` put together into `out`/disccraft-manual.md, with the pictures beside it.
fn write_book(root: &Path, lang: &str, out: &Path) -> Result<PathBuf> {
    let folder = root.join("docs").join("manual").join(lang);
    let chapters = chapters(&folder)?;
    let title = book_title(root, lang);
    // Each chapter's first heading, for links to a whole chapter.
    let first: BTreeMap<&str, String> = chapters
        .iter()
        .filter_map(|chapter| {
            let heading = headings(&chapter.text).into_iter().next()?;
            Some((chapter.name.as_str(), anchor(&heading.1)))
        })
        .collect();
    let mut book = format!(
        "---\ntitle: \"{}\"\nsubtitle: \"{}\"\nlang: {lang}\ntoc: true\n---\n",
        title.replace('"', "'"),
        version(root)
    );
    for (index, chapter) in chapters.iter().enumerate() {
        if index > 0 {
            book.push_str("\n\\newpage\n\n<div style=\"page-break-before: always;\"></div>\n");
        }
        book.push('\n');
        let mut fenced = false;
        for line in chapter.text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            // The navigation between chapters is for reading them one by one.
            if !fenced && line.starts_with("[目次](README.md)")
                || line.starts_with("[Contents](README.md)")
            {
                continue;
            }
            let level = line.chars().take_while(|c| *c == '#').count();
            if !fenced && (1..=6).contains(&level) && line[level..].starts_with(' ') {
                let text = line[level..].trim();
                book.push_str(&format!(
                    "{} {text} {{#{}}}\n",
                    "#".repeat(level),
                    anchor(text)
                ));
                continue;
            }
            book.push_str(&relink(line, &first, lang));
            book.push('\n');
        }
    }
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let pictures = root.join("docs").join("manual").join("images").join(lang);
    if pictures.is_dir() {
        copy_tree(&pictures, &out.join("images"))?;
    }
    // The app's icon, for the title page.
    let icon = root.join("assets").join("app-icon").join("disccraft.png");
    if icon.is_file() {
        let target = out.join(ICON);
        std::fs::copy(&icon, &target).map_err(|error| format!("{}: {error}", target.display()))?;
    }
    let path = out.join("disccraft-manual.md");
    std::fs::write(&path, book).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// `line` with its links made the book's: a chapter's (`NN-name.md#x`) a link inside it, another document's plain
/// text, a picture's path the copy beside the book.
fn relink(line: &str, first: &BTreeMap<&str, String>, lang: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("](") {
        let after = &rest[at + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let target = &after[..end];
        let head = &rest[..at];
        let picture_prefix = format!("../images/{lang}/");
        if let Some(picture) = target.strip_prefix(&picture_prefix) {
            out.push_str(head);
            out.push_str("](images/");
            out.push_str(picture);
            out.push(')');
        } else if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with('#')
        {
            out.push_str(head);
            out.push_str("](");
            out.push_str(target);
            out.push(')');
        } else {
            let (file, heading) = target.split_once('#').unwrap_or((target, ""));
            if let Some(chapter) = first.get(file) {
                let anchor = if heading.is_empty() {
                    chapter.as_str()
                } else {
                    heading
                };
                out.push_str(head);
                out.push_str("](#");
                out.push_str(anchor);
                out.push(')');
            } else if let Some(open) = head.rfind('[') {
                // Another document: its words, without the link.
                out.push_str(&head[..open]);
                out.push_str(&head[open + 1..]);
            } else {
                out.push_str(head);
                out.push_str("](");
                out.push_str(target);
                out.push(')');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The app's icon beside the book, on its title page.
const ICON: &str = "app-icon.png";

/// The words of the book's own pages, by language.
fn book_words(lang: &str) -> BookWords {
    match lang {
        "ja" => BookWords {
            contents: "目次",
            version: "版",
            subtitle: "DVD と Blu-ray のディスクを作る",
            figure: "図",
            result: "結果",
        },
        _ => BookWords {
            contents: "Contents",
            version: "Version",
            subtitle: "Making DVD and Blu-ray discs",
            figure: "Figure",
            result: "Result",
        },
    }
}

/// The words of the book's own pages: the contents' heading, the word before the version, the title page's subtitle,
/// the word before a figure's number, and the word a step's result starts with (in bold).
struct BookWords {
    contents: &'static str,
    version: &'static str,
    subtitle: &'static str,
    figure: &'static str,
    result: &'static str,
}

/// Every figure of `html` named by its number in its chapter (`fig-3-2`; the chapter's number from its heading,
/// `3. …`) and its caption started with that number (`図 3-2`), with markers whose places the PDF tells: under its
/// picture (`-i`), at its end (`-e`), and before what is kept on its page with it (`-b`: the heading and the
/// paragraph that lead to it, else the figure itself).
fn number_figures(html: &str, word: &str) -> String {
    const OPEN: &str = "<figure>";
    const CLOSE: &str = "</figure>";
    const CAPTION: &str = "<figcaption>";
    let mut out = String::with_capacity(html.len() + 16384);
    let mut rest = html;
    let mut chapter = 0;
    let mut figure = 0;
    loop {
        let heading = rest.find("<h1");
        let opening = rest.find(OPEN);
        match (heading, opening) {
            (Some(at), _) if opening.is_none_or(|opening| at < opening) => {
                let after = &rest[at..];
                let number = after
                    .split_once('>')
                    .map(|(_, text)| {
                        text.chars()
                            .take_while(char::is_ascii_digit)
                            .collect::<String>()
                    })
                    .and_then(|digits| digits.parse().ok());
                chapter = number.unwrap_or(chapter + 1);
                figure = 0;
                out.push_str(&rest[..at + 3]);
                rest = &rest[at + 3..];
            }
            (_, Some(at)) => {
                let Some(close) = rest[at..].find(CLOSE).map(|close| at + close) else {
                    break;
                };
                figure += 1;
                let id = format!("{FIGURE}{chapter}-{figure}");
                out.push_str(&rest[..at]);
                mark_kept_start(&mut out, &id);
                let inside = &rest[at + OPEN.len()..close];
                let (picture, caption) =
                    inside.split_at(inside.find(CAPTION).unwrap_or(inside.len()));
                out.push_str(&format!(
                    "<figure id=\"{id}\">{picture}<div class=\"mark\" id=\"{id}-i\"></div>"
                ));
                if let Some(caption) = caption.strip_prefix(CAPTION) {
                    out.push_str(&format!(
                        "{CAPTION}<span class=\"fignum\">{word} {chapter}-{figure}</span>{caption}"
                    ));
                }
                out.push_str(&format!("<div class=\"mark\" id=\"{id}-e\"></div>{CLOSE}"));
                rest = &rest[close + CLOSE.len()..];
            }
            _ => break,
        }
    }
    out.push_str(rest);
    out
}

/// The marker of the figure `id` put into `out` (the book up to the figure) before what the figure keeps on its page
/// (as the CSS does): the paragraph right before it; when that is a list's first step, the list, and the paragraph
/// that leads to the list; the first paragraph under a heading before any of them; and a section's heading right
/// before any of them (`-b2`, `-b3` by the heading; else `-b`).
fn mark_kept_start(out: &mut String, id: &str) {
    /// What the kept run starts with so far.
    enum Start {
        Figure,
        Paragraph,
        List,
    }
    let heading_ending = |text: &str| {
        let text = text.trim_end();
        ["2", "3"].iter().find_map(|level| {
            text.ends_with(&format!("</h{level}>"))
                .then(|| text.rfind(&format!("<h{level}")))
                .flatten()
                .map(|at| (at, *level))
        })
    };
    // A list's start (`<ol>`, `<ol start="3">`, `<ul>`) and its first item at the end of `text`: where it starts.
    let list_opening = |text: &str| {
        let text = text.trim_end().strip_suffix("<li>")?.trim_end();
        let open = text.rfind('<')?;
        let tag = text.get(open..)?;
        ((tag.starts_with("<ol") || tag.starts_with("<ul")) && tag.ends_with('>')).then_some(open)
    };
    // Where the paragraph at the end of `text` starts.
    let paragraph_opening = |text: &str| {
        let text = text.trim_end();
        text.ends_with("</p>")
            .then(|| text.rfind("<p>").max(text.rfind("<p ")))
            .flatten()
    };
    let kept = out.trim_end();
    let mut start = kept.len();
    let mut starts = Start::Figure;
    let mut heading = None;
    while let Some(before) = kept.get(..start) {
        if let Some(found) = heading_ending(before) {
            heading = Some(found);
            break;
        }
        // A paragraph stays with the figure or the list after it; a list's first step with the list; the first
        // paragraph under a heading with what follows it.
        let earlier = match starts {
            Start::Figure | Start::List => {
                paragraph_opening(before).map(|at| (at, Start::Paragraph))
            }
            Start::Paragraph => list_opening(before)
                .map(|at| (at, Start::List))
                .or_else(|| {
                    paragraph_opening(before)
                        .filter(|at| {
                            before
                                .get(..*at)
                                .is_some_and(|text| heading_ending(text).is_some())
                        })
                        .map(|at| (at, Start::Paragraph))
                }),
        };
        let Some((at, next)) = earlier else {
            break;
        };
        start = at;
        starts = next;
    }
    let (at, suffix) = match heading {
        Some((at, level)) => (at, format!("b{level}")),
        None => (start, "b".to_string()),
    };
    out.insert_str(
        at,
        &format!("<div class=\"mark\" id=\"{id}-{suffix}\"></div>"),
    );
}

/// Every paragraph of `html` that starts with `word` in bold (a step's result, `**結果**: …`) given its class.
fn results(html: &str, word: &str) -> String {
    html.replace(
        &format!("<p><strong>{word}</strong>"),
        &format!("<p class=\"result\"><strong>{word}</strong>"),
    )
}

/// The book's look when printed: A4, page numbers, the chapters run on, the pictures framed with their captions, the
/// glossary's index and terms.
const BOOK_CSS: &str = r#"
@page { size: A4; margin: 18mm 16mm 20mm 16mm;
  @bottom-center { content: counter(page); font-size: 9pt; color: #666; } }
@page :first { @bottom-center { content: none; } }
@page { @top-left { font-size: 7.5pt; color: #999; vertical-align: bottom; padding-bottom: 3mm; }
  @top-right { font-size: 7.5pt; color: #777; vertical-align: bottom; padding-bottom: 3mm; } }
html { font-family: "Yu Gothic UI", "Yu Gothic", "Meiryo", "Noto Sans CJK JP", "Hiragino Sans", sans-serif;
  font-size: 10.5pt; line-height: 1.85; color: #222; }
body { margin: 0; }
a { color: #1f5fbf; text-decoration: none; }
h1 { break-after: avoid; font-size: 20pt; line-height: 1.4; margin: 36pt 0 14pt; padding-bottom: 5pt;
  border-bottom: 2.5pt solid #378ef0; }
section.chapter { break-before: page; }
section.chapter > h1 { margin-top: 0; }
section.tight { line-height: 1.72; }
section.tight p { margin-bottom: 7pt; }
section.tight h2 { margin-top: 18pt; }
section.tight figure { margin: 6pt 0 8pt; }
section.tight figure img { max-height: 76mm; }
section.tight pre { line-height: 1.4; }
section.tight li { margin-bottom: 3pt; }
section.tighter { font-size: 10pt; line-height: 1.6; }
section.tighter p { margin-bottom: 5pt; }
section.tighter h2 { margin-top: 14pt; }
section.tighter figure { margin: 5pt 0 6pt; }
section.tighter figure img { max-height: 64mm; }
section.tighter table { font-size: 9pt; }
section.tighter pre { font-size: 8pt; line-height: 1.3; }
section.tighter li { margin-bottom: 2pt; }
.end, .mark { height: 0; }
h2.new-page { break-before: page; margin-top: 0; }
.ends { display: none; }
h2 { font-size: 14.5pt; line-height: 1.4; margin: 26pt 0 8pt; padding-left: 7pt; border-left: 4pt solid #378ef0;
  break-after: avoid; }
h3 { font-size: 12pt; margin: 18pt 0 6pt; break-after: avoid; }
p { margin: 0 0 9pt; }
p, li { orphans: 2; widows: 2; }
ul, ol { padding-left: 18pt; margin: 4pt 0 10pt; }
li { margin-bottom: 4pt; }
li > ul, li > ol { margin: 3pt 0 2pt; }
figure { margin: 10pt 0 12pt; text-align: center; break-inside: avoid; }
p:has(+ figure), p:has(+ table), p:has(+ pre), p:has(+ ul), p:has(+ ol) { break-after: avoid; }
p:has(+ figure), p:has(+ table), p:has(+ pre) { break-inside: avoid; }
h2 + p, h3 + p { break-after: avoid; break-inside: avoid; }
li:not(:has(ul, ol)) { break-inside: avoid; }
figure img { display: block; margin: 0 auto; max-width: 100%; max-height: 88mm; border: 0.75pt solid #bbb; }
figcaption { font-size: 8.5pt; color: #555; margin-top: 3pt; }
.fignum { font-weight: bold; color: #333; margin-right: 6pt; }
ol > li::marker { font-weight: bold; color: #1f5fbf; }
p.result::before { content: "✓"; color: #2e8b3e; font-weight: bold; margin-right: 5pt; }
p.result strong { color: #2e8b3e; }
table { border-collapse: collapse; width: 100%; font-size: 9.5pt; line-height: 1.6; margin: 10pt 0 14pt; }
th, td { border: 0.75pt solid #bbb; padding: 4pt 7pt; vertical-align: top; overflow-wrap: anywhere; }
td:first-child { white-space: nowrap; }
th { background: #eef3fb; }
tr { break-inside: avoid; }
table:not(:has(tr:nth-child(9))) { break-inside: avoid; }
code { font-family: "Consolas", "BIZ UDGothic", monospace; font-size: 9pt; background: #f2f2f2; padding: 0 2pt; }
pre { background: #f5f5f5; padding: 6pt 8pt; white-space: pre-wrap; font-size: 8.5pt; line-height: 1.5;
  break-inside: avoid; }
pre code { background: none; padding: 0; }
blockquote { margin: 10pt 0; padding: 4pt 12pt; border-left: 3pt solid #ccc; color: #555; }
blockquote p:last-child, blockquote ul:last-child { margin-bottom: 2pt; }
blockquote.summary { background: #eef5ff; border: none; border-radius: 6pt; color: #222; padding: 8pt 14pt;
  margin: 4pt 0 16pt; }
blockquote.summary > p:first-child strong { color: #1f5fbf; }
blockquote.tip { background: #f2faf2; border-left: 3pt solid #5aa55a; color: #222; }
blockquote.details { background: #f7f7f7; border-left: 3pt solid #c8c8c8; color: #3c3c3c; font-size: 9.5pt;
  line-height: 1.7; }
blockquote.details li { margin-bottom: 2pt; }
.cover { height: 240mm; display: flex; flex-direction: column; justify-content: center; text-align: center;
  break-after: page; }
.cover .title { font-size: 30pt; font-weight: bold; line-height: 1.4; color: #222; margin: 0 0 6pt; }
.cover .subtitle { font-size: 14pt; color: #378ef0; margin: 0 0 18pt; }
.cover .icon { width: 46mm; height: 46mm; margin: 0 auto 16pt; }
.cover p { font-size: 12pt; color: #555; }
nav.toc { break-after: page; }
nav.toc h1 { margin-top: 0; }
nav.toc .columns { column-count: 2; column-gap: 10mm; }
nav.toc .chapter { break-inside: avoid; margin: 0 0 8pt; }
nav.toc a { display: flex; align-items: baseline; color: #222; }
nav.toc a.chapter { font-weight: bold; font-size: 10pt; line-height: 1.7; }
nav.toc a.section { font-size: 8.5pt; line-height: 1.6; padding-left: 12pt; color: #444; }
nav.toc .dots, .index .dots { flex: 1; border-bottom: 0.75pt dotted #aaa; margin: 0 4pt; position: relative;
  top: -3pt; }
nav.toc .page, .index .page { min-width: 1.8em; text-align: right; }
.index { column-count: 3; column-gap: 7mm; font-size: 8.5pt; line-height: 1.6; margin: 4pt 0 8pt; }
.index .group { break-inside: avoid; margin: 0 0 9pt; }
.index .label { font-weight: bold; color: #1f5fbf; margin: 0 0 2pt; border-bottom: 0.75pt solid #cfdcf0; }
.index a { display: flex; align-items: baseline; color: #222; }
.glossary { break-before: page; }
.glossary h2:first-child { margin-top: 0; }
.glossary .terms { column-count: 2; column-gap: 8mm; }
.glossary .term { break-inside: avoid; margin: 0 0 9pt; }
.glossary h3 { font-size: 11pt; line-height: 1.4; margin: 0 0 2pt; padding-bottom: 1pt; color: #1a3d73;
  border-bottom: 0.75pt solid #cfdcf0; }
.glossary h3 + p:has(> em:only-child) { margin: 0 0 2pt; font-size: 8pt; line-height: 1.4; color: #777; }
.glossary p { font-size: 9.5pt; line-height: 1.7; margin: 0 0 3pt; }
.glossary p.see { font-size: 8pt; line-height: 1.6; color: #666; }
.pageref { font-size: 7.5pt; color: #888; margin-left: 3pt; white-space: nowrap; }
.pageref::before { content: "p."; }
blockquote.summary { break-inside: avoid; }
html:lang(en) { font-family: "Segoe UI", "Helvetica Neue", Arial, "Noto Sans", sans-serif; line-height: 1.55; }
html:lang(en) section.tight { line-height: 1.48; }
html:lang(en) section.tighter { line-height: 1.42; }
html:lang(en) blockquote.details, html:lang(en) .glossary p { line-height: 1.5; }
"#;

/// `text` made safe in HTML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Every picture alone in its paragraph (`<p><img … alt="x" /></p>`) made a figure with its words as the caption.
fn figures(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<p><img ") {
        let after = &rest[at + 3..];
        let Some(end) = after.find("</p>") else {
            break;
        };
        let image = &after[..end];
        let alone = image.matches('<').count() == 1 && image.trim_end().ends_with("/>");
        out.push_str(&rest[..at]);
        if alone {
            let caption = image
                .split_once("alt=\"")
                .and_then(|(_, alt)| alt.split_once('"'))
                .map(|(alt, _)| alt)
                .unwrap_or_default();
            out.push_str("<figure>");
            out.push_str(image);
            if !caption.is_empty() {
                out.push_str("<figcaption>");
                out.push_str(caption);
                out.push_str("</figcaption>");
            }
            out.push_str("</figure>");
        } else {
            out.push_str("<p>");
            out.push_str(image);
            out.push_str("</p>");
        }
        rest = &after[end + 4..];
    }
    out.push_str(rest);
    out
}

/// The kinds of callout (a quote that starts with these words in bold): the chapter's summary, a tip, details in
/// small type.
const CALLOUTS: [(&str, &str); 6] = [
    ("この章でできること", "summary"),
    ("In this chapter", "summary"),
    ("ヒント", "tip"),
    ("Tip", "tip"),
    ("詳しく", "details"),
    ("Details", "details"),
];

/// Every quote that starts with a callout's words (`<blockquote>` then `<p><strong>…</strong>`) given its class.
fn callouts(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<blockquote>") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "<blockquote>".len()..];
        let opening = after
            .trim_start()
            .strip_prefix("<p><strong>")
            .and_then(|words| {
                CALLOUTS
                    .iter()
                    .find(|(start, _)| words.starts_with(start))
                    .map(|(_, class)| *class)
            });
        match opening {
            Some(class) => out.push_str(&format!("<blockquote class=\"{class}\">")),
            None => out.push_str("<blockquote>"),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Whether `c` is a Japanese (or other wide) character, between which a line break is no space.
fn wide(c: char) -> bool {
    !c.is_ascii() && !c.is_whitespace()
}

/// The soft line breaks of the source between two Japanese characters left out: the manual wraps its lines anywhere,
/// and a browser draws such a break as a space in the middle of a Japanese sentence. A break next to a word in Latin
/// letters stays a space, as the manual writes one there.
fn join_japanese_lines(events: Vec<pulldown_cmark::Event<'_>>) -> Vec<pulldown_cmark::Event<'_>> {
    use pulldown_cmark::Event;
    let last = |event: &Event<'_>| match event {
        Event::Text(text) | Event::Code(text) => text.chars().last(),
        _ => None,
    };
    let first = |event: &Event<'_>| match event {
        Event::Text(text) | Event::Code(text) => text.chars().next(),
        _ => None,
    };
    let mut out = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        if matches!(event, Event::SoftBreak) {
            let before = index
                .checked_sub(1)
                .and_then(|at| events.get(at))
                .and_then(last);
            let after = events.get(index + 1).and_then(first);
            if before.is_some_and(wide) && after.is_some_and(wide) {
                continue;
            }
        }
        out.push(event.clone());
    }
    out
}

/// The contents page: each chapter with its sections, each entry with a dotted leader and its page (`pages`, by
/// heading id; none on the first printing), in two columns that never split a chapter.
fn contents(body: &str, contents_word: &str, pages: &BTreeMap<String, u32>) -> String {
    let entry = |class: &str, id: &str, words: &str| {
        let page = pages.get(id).map(u32::to_string).unwrap_or_default();
        format!(
            "<a class=\"{class}\" href=\"#{id}\"><span class=\"words\">{words}</span><span class=\"dots\"></span><span class=\"page\">{page}</span></a>",
            id = escape(id),
            words = escape(words),
        )
    };
    let mut toc = format!("<nav class=\"toc\"><h1>{contents_word}</h1><div class=\"columns\">");
    let mut open = false;
    for (level, heading) in headings(body) {
        let (words, id) = match heading.rsplit_once(" {#") {
            Some((words, id)) => (words, id.trim_end_matches('}')),
            None => (heading.as_str(), ""),
        };
        match level {
            1 => {
                if open {
                    toc.push_str("</div>");
                }
                toc.push_str("<div class=\"chapter\">");
                toc.push_str(&entry("chapter", id, words));
                open = true;
            }
            2 => toc.push_str(&entry("section", id, words)),
            _ => {}
        }
    }
    if open {
        toc.push_str("</div>");
    }
    toc.push_str("</div></nav>");
    toc
}

/// `html` with what is between the markers `<!-- name -->` and `<!-- /name -->` (a chapter's, invisible on GitHub)
/// given to `change`, the markers left out.
fn region(html: &str, name: &str, change: impl Fn(&str) -> String) -> String {
    let open = format!("<!-- {name} -->");
    let close = format!("<!-- /{name} -->");
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(end) = after.find(&close) else {
            break;
        };
        out.push_str(&rest[..at]);
        out.push_str(&change(&after[..end]));
        rest = &after[end + close.len()..];
    }
    out.push_str(rest);
    out
}

/// A link inside the book: where it is in the HTML, the heading id it goes to, its words.
struct InternalLink<'a> {
    end: usize,
    href: &'a str,
    words: &'a str,
}

/// The links of `html` to its own headings (`<a href="#id">words</a>`).
fn internal_links(html: &str) -> Vec<InternalLink<'_>> {
    const OPEN: &str = "<a href=\"#";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = html.get(from..).and_then(|rest| rest.find(OPEN)) {
        let start = from + at;
        let href_start = start + OPEN.len();
        let Some(rest) = html.get(href_start..) else {
            break;
        };
        let (Some(href_len), Some(words_at), Some(words_len)) =
            (rest.find('"'), rest.find('>'), rest.find("</a>"))
        else {
            break;
        };
        if words_at > words_len {
            from = href_start;
            continue;
        }
        found.push(InternalLink {
            end: href_start + words_len + "</a>".len(),
            href: &rest[..href_len],
            words: &rest[words_at + 1..words_len],
        });
        from = href_start + words_len + "</a>".len();
    }
    found
}

/// The page of the heading a link goes to (its `href` percent-encoded or not).
fn page_of(href: &str, pages: &BTreeMap<String, u32>) -> Option<u32> {
    pages
        .get(href)
        .or_else(|| pages.get(&unescaped(href.as_bytes(), b'%')))
        .copied()
}

/// The glossary's index (`<!-- index -->`): a paragraph a group (its name in bold, then its terms linked) made a
/// list in three columns, each term with a dotted leader and its page.
fn index(html: &str, pages: &BTreeMap<String, u32>) -> String {
    let mut out = String::from("<div class=\"index\">");
    for paragraph in html.split("<p>").skip(1) {
        let label = paragraph
            .split_once("<strong>")
            .and_then(|(_, rest)| rest.split_once("</strong>"))
            .map_or("", |(label, _)| label);
        out.push_str(&format!(
            "<div class=\"group\"><p class=\"label\">{label}</p>"
        ));
        for link in internal_links(paragraph) {
            let page = page_of(link.href, pages)
                .map(|page| page.to_string())
                .unwrap_or_default();
            out.push_str(&format!(
                "<a href=\"#{}\"><span class=\"words\">{}</span><span class=\"dots\"></span><span class=\"page\">{page}</span></a>",
                link.href, link.words
            ));
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
    out
}

/// The glossary's terms (`<!-- glossary -->`), from a new page: each group (a second-level heading) in two columns
/// under its heading, each term (a third-level heading and what follows it) kept together, and the links of its last
/// line (`→ …`, where to read more) given their pages.
fn glossary(html: &str, pages: &BTreeMap<String, u32>) -> String {
    let mut out = String::from("<div class=\"glossary\">");
    let mut groups = html.split("<h2");
    if let Some(before) = groups.next() {
        out.push_str(before);
    }
    for group in groups {
        let Some((heading, terms)) = group.split_once("</h2>") else {
            out.push_str("<h2");
            out.push_str(group);
            continue;
        };
        out.push_str(&format!("<h2{heading}</h2><div class=\"terms\">"));
        let mut terms = terms.split("<h3");
        if let Some(before) = terms.next() {
            out.push_str(before);
        }
        for term in terms {
            out.push_str("<div class=\"term\">");
            out.push_str(&see_pages(&format!("<h3{term}"), pages));
            out.push_str("</div>");
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
    out
}

/// A glossary term's HTML with its `→` line given a class, and each link there its page.
fn see_pages(html: &str, pages: &BTreeMap<String, u32>) -> String {
    let Some(at) = html.find("<p>→ ") else {
        return html.to_string();
    };
    let line = &html[at..];
    let mut out = String::with_capacity(html.len() + 200);
    out.push_str(&html[..at]);
    out.push_str("<p class=\"see\">");
    out.push_str(&with_pages(&line["<p>".len()..], pages));
    out
}

/// `html` with every link to a heading followed by that heading's page (a list of where to read, `<!-- pages -->`).
fn with_pages(html: &str, pages: &BTreeMap<String, u32>) -> String {
    let mut out = String::with_capacity(html.len() + 512);
    let mut used = 0;
    for link in internal_links(html) {
        out.push_str(&html[used..link.end]);
        if let Some(page) = page_of(link.href, pages) {
            out.push_str(&format!("<span class=\"pageref\">{page}</span>"));
        }
        used = link.end;
    }
    out.push_str(&html[used..]);
    out
}

/// The id prefix of the marker at the end of each chapter.
const END: &str = "end-";

/// The chapters of `html` (each from its `<h1`) as sections that start a page, each on pages of its own name whose
/// head carries the book's title (`book`) and the chapter's (the CSS of those pages is the second part), set tighter
/// at the levels of `fits` (by the chapter's id; 1 tight, 2 tighter), each ending with a marker whose page and height
/// the PDF tells: the browser names only the places links go to, so a hidden list links to the markers (and to the
/// figures' markers). The figures of the layout's shrinks are held to their heights (at most as high as their
/// chapter's level allows) and the sections of its breaks start a page.
fn chapter_sections(html: &str, book: &str, layout: &Layout) -> (String, String) {
    let mut out = String::with_capacity(html.len() + 8192);
    let mut css = String::new();
    let mut ends = String::new();
    let mut parts = html.split("<h1");
    if let Some(before) = parts.next() {
        out.push_str(before);
    }
    for (number, part) in parts.enumerate() {
        let id = part
            .strip_prefix(" id=\"")
            .and_then(|rest| rest.split_once('"'))
            .map_or("", |(id, _)| id);
        let level = layout.fits.get(id).map_or(0, |fit| fit.level);
        let class = match level {
            0 => "",
            1 => " tight",
            _ => " tighter",
        };
        let mut part = part.to_string();
        for section in &layout.breaks {
            part = part.replace(
                &format!("<h2 id=\"{section}\">"),
                &format!("<h2 id=\"{section}\" class=\"new-page\">"),
            );
        }
        let part = part.as_str();
        // The highest a picture is in a chapter set tighter (as the CSS says).
        let most = match level {
            0 => f32::MAX,
            1 => 76.0 * 72.0 / 25.4,
            _ => 64.0 * 72.0 / 25.4,
        };
        let mut marks = part;
        while let Some(at) = marks.find(&format!(" id=\"{FIGURE}")) {
            let named = &marks[at + 5..];
            let Some((mark, _)) = named.split_once('"') else {
                break;
            };
            ends.push_str(&format!("<a href=\"#{mark}\"></a>"));
            if let Some(height) = layout.shrinks.get(mark).and_then(|shrink| shrink.height) {
                css.push_str(&format!(
                    "#{mark} img {{ max-height: {:.1}pt; }}\n",
                    height.min(most)
                ));
            }
            marks = named;
        }
        let heading = part
            .split_once('>')
            .and_then(|(_, rest)| rest.split_once("</h1>"))
            .map_or(String::new(), |(heading, _)| plain_text(heading));
        out.push_str(&format!(
            "<section class=\"chapter{class}\" style=\"page: chapter{number}\"><h1{part}<div class=\"end\" id=\"{END}{id}\"></div></section>\n"
        ));
        css.push_str(&format!(
            "@page chapter{number} {{ @top-left {{ content: \"{}\"; }} @top-right {{ content: \"{}\"; }} }}\n",
            css_string(book),
            css_string(&heading)
        ));
        ends.push_str(&format!("<a href=\"#{END}{id}\"></a>"));
    }
    out.push_str(&format!("<nav class=\"ends\">{ends}</nav>"));
    (out, css)
}

/// The text of a piece of HTML: its tags left out, its entities undone.
fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// `text` as the inside of a CSS string (`"…"`).
fn css_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

/// How a chapter is set so that its last page is not a few lines alone: the level it is set at (0 as the rest, 1
/// tight, 2 tighter), the pages it ran onto at level 0, and whether that is decided.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Fit {
    level: u8,
    pages: u32,
    settled: bool,
}

/// A chapter whose last page is filled less than this is set tighter, to bring those lines back.
const SHORT_TAIL: f32 = 0.3;

/// The top and the bottom of the text of a page as the PDF's destinations measure (points from the bottom): the
/// browser counts from the top of the text as if it were the top of the page, so the top margin is left out (A4, the
/// book's margins).
const TEXT_TOP: f32 = 841.92;
const TEXT_BOTTOM: f32 = TEXT_TOP - (841.92 - (18.0 + 20.0) * 72.0 / 25.4);

/// How far down the text of its page a height of a destination is: 0 the top of the text, 1 its bottom.
fn depth(y: f32) -> f32 {
    ((TEXT_TOP - y) / (TEXT_TOP - TEXT_BOTTOM)).clamp(0.0, 1.0)
}

/// The id prefix of a figure (`fig-3-2`, the second of chapter 3) and of its markers.
const FIGURE: &str = "fig-";

/// How a figure is made smaller so that it is not sent to the next page for want of a little room: its picture's
/// height as first printed (with its frame) and the height it is held to (inside the frame; points), the tries so
/// far, and whether that is decided.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Shrink {
    original: f32,
    height: Option<f32>,
    tries: u8,
    settled: bool,
}

/// A figure is made smaller to at most this part of its height to stay on its page (about as high as in a chapter
/// set tighter), and tried twice.
const LEAST_SCALE: f32 = 0.7;
const SHRINK_TRIES: u8 = 2;

/// A figure counts as sent on when it starts this far down its page at most (under its heading and paragraph).
const SENT_DEPTH: f32 = 0.4;

/// The markers before what a figure keeps on its page with it, by what that starts with (a figure, a heading of a
/// section's part, of a section), and the room taken above that more than above the marker (its top margin, less the
/// one of the paragraph before; points).
const KEPT_STARTS: [(&str, f32); 3] = [("b", 1.0), ("b3", 9.0), ("b2", 17.0)];

/// A little more taken off a figure than measured, and the picture's frame (its height is the picture's inside it).
const SHRINK_SLACK: f32 = 2.0;
const FRAME: f32 = 1.5;

/// Where a figure and its markers landed: its top, the bottom of its picture, its end, and the marker before what it
/// keeps on its page with it (with the room taken above that).
#[derive(Clone, Copy)]
struct Landed {
    top: Place,
    picture: Place,
    end: Place,
    kept: Option<(Place, f32)>,
}

/// Where the figure `id` and its markers landed (`places`).
fn landed(places: &BTreeMap<String, Place>, id: &str) -> Option<Landed> {
    let mark = |suffix: &str| places.get(&format!("{id}-{suffix}")).copied();
    Some(Landed {
        top: places.get(id).copied()?,
        picture: mark("i")?,
        end: mark("e")?,
        kept: KEPT_STARTS
            .iter()
            .find_map(|(suffix, margin)| Some((mark(suffix)?, *margin))),
    })
}

/// The figures of `places` in their order: (chapter, number in it, id).
fn figures_in_order(places: &BTreeMap<String, Place>) -> Vec<(u32, u32, &str)> {
    let mut figures: Vec<(u32, u32, &str)> = places
        .keys()
        .filter_map(|name| {
            let (chapter, number) = name.strip_prefix(FIGURE)?.split_once('-')?;
            Some((chapter.parse().ok()?, number.parse().ok()?, name.as_str()))
        })
        .collect();
    figures.sort_unstable();
    figures
}

/// The next step of fitting the figures and the sections from where they landed (`places`; `leads`, the first figure
/// of each section): in each chapter, the first figure not yet decided (what comes after it moves with it).
/// - Sent to the next page with room left on the one before, it is held, with the figures on that page, to the heights
///   that bring it back there (all made smaller by the same part), when none is much smaller than at first; kept when
///   that brings it back, else tried once more.
/// - Else, when it is a section's first figure and the section's heading was left low on the page before
///   (`LOW_HEADING`, or `LOW_SHORT_HEADING` for a short section) and both fit on one page, the section starts the next.
/// - Else it stays where it is (the figures made smaller for it set back).
///
/// Whether anything changed.
fn arrange(
    places: &BTreeMap<String, Place>,
    leads: &[(String, String)],
    layout: &mut Layout,
) -> bool {
    let headings: BTreeMap<&str, &str> = leads
        .iter()
        .map(|(heading, figure)| (figure.as_str(), heading.as_str()))
        .collect();
    let figures = figures_in_order(places);
    let mut busy = BTreeSet::new();
    let mut changed = false;
    for &(chapter, _, id) in &figures {
        if busy.contains(&chapter) || layout.shrinks.get(id).is_some_and(|shrink| shrink.settled) {
            continue;
        }
        let Some(figure) = landed(places, id) else {
            continue;
        };
        let whole = figure.picture.page == figure.top.page && figure.end.page == figure.top.page;
        let sent = whole
            && figure
                .kept
                .is_some_and(|(before, _)| before.page + 1 == figure.top.page)
            && depth(figure.top.y) < SENT_DEPTH;
        let split = headings
            .get(id)
            .filter(|heading| !layout.breaks.contains(**heading))
            .and_then(|heading| Some((*heading, *places.get(*heading)?)))
            .filter(|(_, start)| whole && start.page + 1 == figure.top.page);
        if !sent && split.is_none() {
            // As printed it stays where it is: a height that brought it back is kept.
            if let Some(shrink) = layout.shrinks.get_mut(id) {
                shrink.settled = true;
            }
            continue;
        }
        if sent && shrink_to_fit(places, &figures, id, &figure, layout) {
            busy.insert(chapter);
            changed = true;
            continue;
        }
        let moved = split.filter(|(_, start)| {
            let lead =
                ((start.y - TEXT_BOTTOM) + (TEXT_TOP - figure.end.y)) / (TEXT_TOP - TEXT_BOTTOM);
            let low = depth(start.y);
            low > LOW_HEADING && lead <= 1.0 || low > LOW_SHORT_HEADING && lead <= SHORT_LEAD
        });
        let undone = set_back(id, layout);
        let height = figure.top.y - figure.picture.y;
        layout
            .shrinks
            .entry(id.to_string())
            .or_insert(Shrink {
                original: height,
                ..Shrink::default()
            })
            .settled = true;
        if let Some((heading, _)) = moved {
            layout.breaks.insert(heading.to_string());
        }
        if undone || moved.is_some() {
            busy.insert(chapter);
            changed = true;
        }
    }
    changed
}

/// Holds the figure `id` (landed as `figure`, sent to the next page) and the figures wholly on the page before (of
/// `figures`) to the heights that bring it back there, all made smaller by the same part, when none is then much
/// smaller than at first and it has tries left; keeps the heights the others had, to set them back. Whether it did.
fn shrink_to_fit(
    places: &BTreeMap<String, Place>,
    figures: &[(u32, u32, &str)],
    id: &str,
    figure: &Landed,
    layout: &mut Layout,
) -> bool {
    let Some((before, margin)) = figure.kept else {
        return false;
    };
    let height = figure.top.y - figure.picture.y;
    let others: Vec<(&str, f32)> = figures
        .iter()
        .filter(|(_, _, other)| *other != id)
        .filter_map(|(_, _, other)| {
            let landed = landed(places, other)?;
            [landed.top.page, landed.picture.page, landed.end.page]
                .iter()
                .all(|page| *page == before.page)
                .then_some((*other, landed.top.y - landed.picture.y))
        })
        .collect();
    let lacking = (TEXT_TOP - figure.end.y) - (before.y - TEXT_BOTTOM - margin) + SHRINK_SLACK;
    let total = height + others.iter().map(|(_, height)| height).sum::<f32>();
    let scale = 1.0 - lacking / total;
    let held = |height: f32| height * scale - FRAME;
    let own = layout.shrinks.get(id).copied().unwrap_or_default();
    let fits = lacking > SHRINK_SLACK
        && own.tries < SHRINK_TRIES
        && own.height.is_none_or(|was| held(height) < was - 0.5)
        && std::iter::once((id, height))
            .chain(others.iter().copied())
            .all(|(figure, height)| {
                let original = layout
                    .shrinks
                    .get(figure)
                    .map_or(height, |shrink| shrink.original);
                height * scale >= original * LEAST_SCALE
            });
    if !fits {
        return false;
    }
    let shrink = layout.shrinks.entry(id.to_string()).or_insert(Shrink {
        original: height,
        ..Shrink::default()
    });
    shrink.height = Some(held(height));
    shrink.tries += 1;
    let mut had = Vec::new();
    for (other, height) in others {
        let shrink = layout.shrinks.entry(other.to_string()).or_insert(Shrink {
            original: height,
            settled: true,
            ..Shrink::default()
        });
        had.push((other.to_string(), shrink.height));
        shrink.height = Some(held(height));
    }
    // The heights before the first try are the ones to set back to.
    layout.shared.entry(id.to_string()).or_insert(had);
    true
}

/// Sets the figure `id` back to its own height, and the figures made smaller with it to the heights they had. Whether
/// anything changed.
fn set_back(id: &str, layout: &mut Layout) -> bool {
    let mut undone = layout
        .shrinks
        .get_mut(id)
        .and_then(|shrink| shrink.height.take())
        .is_some();
    for (other, height) in layout.shared.remove(id).unwrap_or_default() {
        if let Some(shrink) = layout.shrinks.get_mut(&other) {
            undone |= shrink.height != height;
            shrink.height = height;
        }
    }
    undone
}

/// Forgets how the figures and the sections of the chapters `chapters` (by their numbers) were fitted, to fit them
/// again (as those chapters are now set). Whether there was any.
fn unarrange(layout: &mut Layout, chapters: &BTreeSet<u32>, leads: &[(String, String)]) -> bool {
    let of = |figure: &str| -> bool {
        figure
            .strip_prefix(FIGURE)
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(chapter, _)| chapter.parse().ok())
            .is_some_and(|chapter| chapters.contains(&chapter))
    };
    let before = (layout.shrinks.len(), layout.breaks.len());
    layout.shrinks.retain(|figure, _| !of(figure));
    layout.shared.retain(|figure, _| !of(figure));
    layout.breaks.retain(|heading| {
        !leads
            .iter()
            .any(|(section, figure)| section == heading && of(figure))
    });
    before != (layout.shrinks.len(), layout.breaks.len())
}

/// The next step of fitting the chapters from where their starts and ends landed (`places`): a chapter whose last
/// page holds a short tail is tried tight, then tighter, and kept so only when that saves the page (else it goes
/// back). Whether anything changed.
fn refit(places: &BTreeMap<String, Place>, fits: &mut BTreeMap<String, Fit>) -> bool {
    let mut changed = false;
    for (name, end) in places {
        let Some(chapter) = name.strip_prefix(END) else {
            continue;
        };
        let Some(start) = places.get(chapter) else {
            continue;
        };
        let pages = end.page.saturating_sub(start.page);
        let fit = fits.entry(chapter.to_string()).or_default();
        if fit.settled {
            continue;
        }
        if fit.level == 0 {
            if pages > 0 && depth(end.y) < SHORT_TAIL {
                *fit = Fit {
                    level: 1,
                    pages,
                    settled: false,
                };
                changed = true;
            } else {
                fit.settled = true;
            }
        } else if pages < fit.pages {
            fit.settled = true;
        } else if fit.level < 2 {
            fit.level += 1;
            changed = true;
        } else {
            fit.level = 0;
            fit.settled = true;
            changed = true;
        }
    }
    changed
}

/// What the printing of the PDF is fitting: the figures and the sections, the chapters' last pages, then the figures
/// and the sections again (in the chapters set tighter).
#[derive(Clone, Copy, PartialEq)]
enum Step {
    Arrange,
    Fit,
    Rearrange,
}

/// How the book is set beyond its CSS, from where things landed when it was printed: the chapters set tighter (by
/// their ids), the figures held smaller, and the sections started on a new page (by their headings' ids).
#[derive(Debug, Default)]
struct Layout {
    fits: BTreeMap<String, Fit>,
    shrinks: BTreeMap<String, Shrink>,
    breaks: BTreeSet<String>,
    /// The heights the figures made smaller for a figure had before (by that figure's id), to set them back.
    shared: BTreeMap<String, Vec<(String, Option<f32>)>>,
}

/// The HTML page of the book as written: its path, how many figures it holds smaller and how many sections it starts
/// on a new page, and the first figure of each section (the ids of its heading and of the figure).
struct Written {
    path: PathBuf,
    smaller: usize,
    new_pages: usize,
    leads: Vec<(String, String)>,
}

/// The first figure of each section of `html` (a part under a `<h2`, to the next heading of that level or above): the
/// ids of the section's heading and of the figure.
fn section_leads(html: &str) -> Vec<(String, String)> {
    const TAGS: [&str; 3] = ["<h1", "<h2 id=\"", "<figure id=\""];
    let mut leads = Vec::new();
    let mut section = None;
    let mut rest = html;
    while let Some((at, tag)) = TAGS
        .iter()
        .filter_map(|tag| rest.find(tag).map(|at| (at, *tag)))
        .min()
    {
        let after = &rest[at + tag.len()..];
        let id = after.split_once('"').map_or("", |(id, _)| id);
        match tag {
            "<h1" => section = None,
            "<h2 id=\"" => section = Some(id),
            _ => {
                if let Some(heading) = section.take() {
                    leads.push((heading.to_string(), id.to_string()));
                }
            }
        }
        rest = after;
    }
    leads
}

/// A section's heading this far down its page at least, with the section's first figure sent to the next page, starts
/// that page instead (when the heading and the figure then fit on it); a short section (from its heading to the end of
/// its figure this much of a page at most, as a step-by-step one) already from halfway down.
const LOW_HEADING: f32 = 0.7;
const LOW_SHORT_HEADING: f32 = 0.5;
const SHORT_LEAD: f32 = 0.8;

/// The book `book` (its Markdown) as one HTML page beside it: a title page, the contents (chapters and their
/// sections, linked, with the pages of `pages`), then the chapters, each from a new page (set as `layout` says).
fn write_html(
    book: &Path,
    lang: &str,
    (title, version): (&str, &str),
    pages: &BTreeMap<String, u32>,
    layout: &Layout,
) -> Result<Written> {
    use pulldown_cmark::{Options, Parser, html};
    let text =
        std::fs::read_to_string(book).map_err(|error| format!("{}: {error}", book.display()))?;
    // The YAML title block and the page breaks are for other converters: the chapters run on, CSS keeping a heading
    // with what follows it.
    let body: String = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(text.as_str(), |(_, rest)| rest)
        .lines()
        .filter(|line| *line != "\\newpage" && !line.starts_with("<div style=\"page-break-before"))
        .collect::<Vec<_>>()
        .join("\n");
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_HEADING_ATTRIBUTES | Options::ENABLE_STRIKETHROUGH;
    let mut chapters = String::new();
    html::push_html(
        &mut chapters,
        join_japanese_lines(Parser::new_ext(&body, options).collect()).into_iter(),
    );
    let words = book_words(lang);
    let toc = contents(&body, words.contents, pages);
    let icon = if book.with_file_name(ICON).is_file() {
        format!("<img class=\"icon\" src=\"{ICON}\" alt=\"\">")
    } else {
        String::new()
    };
    let chapters = region(&chapters, "index", |part| index(part, pages));
    let chapters = region(&chapters, "glossary", |part| glossary(part, pages));
    let chapters = region(&chapters, "pages", |part| with_pages(part, pages));
    let chapters = results(
        &number_figures(&figures(&chapters), words.figure),
        words.result,
    );
    let leads = section_leads(&chapters);
    let (chapters, chapter_pages) = chapter_sections(&callouts(&chapters), title, layout);
    let new_pages = chapters.matches(" class=\"new-page\"").count();
    let page = format!(
        "<!doctype html>\n<html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{title}</title><style>{BOOK_CSS}{chapter_pages}</style></head>\n<body>\n<section class=\"cover\">{icon}<p class=\"title\">{title}</p><p class=\"subtitle\">{subtitle}</p><p>{version_word} {version}</p></section>\n{toc}\n<main>\n{chapters}\n</main>\n</body></html>\n",
        title = escape(title),
        subtitle = words.subtitle,
        version_word = words.version,
        version = escape(version),
    );
    let path = book.with_extension("html");
    std::fs::write(&path, page).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Written {
        smaller: chapter_pages.matches(" img { max-height:").count(),
        new_pages,
        leads,
        path,
    })
}

/// The text of object `number` of the PDF `pdf` (between `N 0 obj` and `endobj`), as the browsers write them:
/// uncompressed dictionaries.
fn pdf_object(pdf: &[u8], number: u32) -> Option<&[u8]> {
    let start = format!("\n{number} 0 obj\n");
    let at = find(pdf, start.as_bytes())? + start.len();
    let end = find(pdf.get(at..)?, b"\nendobj")?;
    pdf.get(at..at + end)
}

/// Where `needle` first is in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len().max(1))
        .position(|window| window == needle)
}

/// The object number of the reference after `key` in `dictionary` (`/Key 12 0 R`).
fn reference(dictionary: &[u8], key: &[u8]) -> Option<u32> {
    let at = find(dictionary, key)? + key.len();
    let digits: String = dictionary
        .get(at..)?
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take_while(|byte| byte.is_ascii_digit())
        .map(|byte| char::from(*byte))
        .collect();
    digits.parse().ok()
}

/// The object numbers of the references in a PDF array (`[1 0 R 2 0 R]`): every number followed by ` 0 R`.
fn references(array: &[u8]) -> Vec<u32> {
    let text = String::from_utf8_lossy(array);
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(3)
        .filter(|three| three[1] == "0" && three[2].starts_with('R'))
        .filter_map(|three| three[0].trim_start_matches('[').parse().ok())
        .collect()
}

/// The pages of the page tree under `node`, in order (at most `budget` nodes are read).
fn page_order(pdf: &[u8], node: u32, out: &mut Vec<u32>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let Some(object) = pdf_object(pdf, node) else {
        return;
    };
    match find(object, b"/Kids") {
        Some(at) => {
            let kids = object.get(at..).unwrap_or_default();
            let end = kids
                .iter()
                .position(|byte| *byte == b']')
                .unwrap_or(kids.len());
            for kid in references(kids.get(..end).unwrap_or_default()) {
                page_order(pdf, kid, out, budget);
            }
        }
        None => out.push(node),
    }
}

/// `bytes` with its escapes (`mark` and two hexadecimal digits) undone.
fn unescaped(bytes: &[u8], mark: u8) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes.get(index), hex) {
            (Some(byte), Some(value)) if *byte == mark => {
                out.push(value);
                index += 3;
            }
            (Some(byte), _) => {
                out.push(*byte);
                index += 1;
            }
            (None, _) => break,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A PDF name's text: its `#XX` escapes undone, then its percent-encoding (the browser writes an id that way).
fn pdf_name(raw: &[u8]) -> String {
    unescaped(unescaped(raw, b'#').as_bytes(), b'%')
}

/// Where a named destination of the PDF is: its page (1 = the first) and its height on the page (points from the
/// bottom, as PDF measures).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    page: u32,
    y: f32,
}

/// The place of each named destination of the PDF `pdf`: the ids of the headings (and of the chapters' ends) that
/// links go to.
fn destinations(pdf: &[u8]) -> BTreeMap<String, Place> {
    let mut found = BTreeMap::new();
    let Some(catalog_at) = find(pdf, b"/Type /Catalog") else {
        return found;
    };
    let catalog = pdf.get(catalog_at..).unwrap_or_default();
    let catalog = catalog
        .get(..find(catalog, b"endobj").unwrap_or(catalog.len()))
        .unwrap_or_default();
    let (Some(pages), Some(dests)) = (reference(catalog, b"/Pages"), reference(catalog, b"/Dests"))
    else {
        return found;
    };
    let mut order = Vec::new();
    let mut budget = 100_000;
    page_order(pdf, pages, &mut order, &mut budget);
    let page_of: BTreeMap<u32, u32> = order
        .iter()
        .enumerate()
        .filter_map(|(index, object)| Some((*object, u32::try_from(index + 1).ok()?)))
        .collect();
    let Some(dictionary) = pdf_object(pdf, dests) else {
        return found;
    };
    // `/name [12 0 R /XYZ 0 841.9 0]`, one after another.
    let mut rest = dictionary;
    while let Some(slash) = rest.iter().position(|byte| *byte == b'/') {
        let after = rest.get(slash + 1..).unwrap_or_default();
        let name_end = after
            .iter()
            .position(|byte| {
                byte.is_ascii_whitespace() || *byte == b'[' || *byte == b'/' || *byte == b'>'
            })
            .unwrap_or(after.len());
        let name = after.get(..name_end).unwrap_or_default();
        let tail = after.get(name_end..).unwrap_or_default();
        let target = tail
            .iter()
            .position(|byte| *byte == b'[')
            .filter(|open| {
                tail.get(..*open)
                    .is_some_and(|gap| gap.iter().all(u8::is_ascii_whitespace))
            })
            .and_then(|open| {
                let array = tail.get(open..)?;
                let close = array.iter().position(|byte| *byte == b']')?;
                let array = array.get(..close)?;
                // `/XYZ left top zoom`.
                let y = find(array, b"/XYZ")
                    .and_then(|at| {
                        String::from_utf8_lossy(array.get(at + 4..)?)
                            .split_whitespace()
                            .nth(1)?
                            .parse::<f32>()
                            .ok()
                    })
                    .unwrap_or(0.0);
                Some((references(array), y, open + close + 1))
            });
        match target {
            Some((objects, y, used)) => {
                if let Some(page) = objects.first().and_then(|object| page_of.get(object)) {
                    found.insert(pdf_name(name), Place { page: *page, y });
                }
                rest = tail.get(used..).unwrap_or_default();
            }
            None => rest = tail,
        }
    }
    found
}

/// A browser that prints a page as a PDF without a window: `DISCCRAFT_BROWSER`, else Edge or Chrome where they are
/// installed, else Chromium, Chrome or Edge on the PATH.
fn browser() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("DISCCRAFT_BROWSER") {
        return Some(PathBuf::from(named));
    }
    let installed = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    if let Some(found) = installed
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    {
        return Some(found);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|folder| {
        [
            "chromium",
            "chromium-browser",
            "google-chrome",
            "chrome",
            "microsoft-edge",
            "msedge",
        ]
        .iter()
        .map(|name| folder.join(name))
        .find(|candidate| candidate.is_file())
    })
}

/// Prints the HTML page `page` as the PDF `target` with `browser`, without a window and with no header or footer of
/// the browser's own (the page numbers are the page's).
fn print_pdf(browser: &Path, page: &Path, target: &Path) -> Result<()> {
    let page = std::path::absolute(page).map_err(|error| error.to_string())?;
    let url = format!(
        "file:///{}",
        page.to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches('/')
    );
    let _ = std::fs::remove_file(target);
    let status = std::process::Command::new(browser)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-pdf-header-footer",
            "--generate-pdf-document-outline",
            "--no-first-run",
        ])
        .arg(format!("--print-to-pdf={}", target.display()))
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("{}: {error}", browser.display()))?;
    if !status.success() || !target.is_file() {
        return Err(format!(
            "{} did not print the PDF ({status})",
            browser.display()
        ));
    }
    Ok(())
}

/// Makes the PDF of `book`: its HTML page printed by a browser, then printed again with the pages each heading landed
/// on written into the contents (a browser cannot number them itself), with the figures sent to the next page for
/// want of a little room made that much smaller, the sections whose heading was left low on a page without their
/// figure started on the next, then with the chapters whose last page was a few lines set a little tighter where that
/// saves the page (and their figures and sections looked at again), until all hold (thirty-two printings at most).
fn pdf(book: &Path, lang: &str, title: &str, version: &str) -> Result<()> {
    let target = book.with_extension("pdf");
    let browser = browser().ok_or(
        "no browser to print the PDF with: install Edge, Chrome or Chromium, or name one in DISCCRAFT_BROWSER",
    )?;
    let mut pages = BTreeMap::new();
    let mut layout = Layout::default();
    let mut step = Step::Arrange;
    let mut written = None;
    for _ in 0..32 {
        let page = write_html(book, lang, (title, version), &pages, &layout)?;
        print_pdf(&browser, &page.path, &target)?;
        let printed =
            std::fs::read(&target).map_err(|error| format!("{}: {error}", target.display()))?;
        let places = destinations(&printed);
        if places.is_empty() {
            println!(
                "manual ({lang}): the PDF names no headings, so the contents have no page numbers"
            );
            break;
        }
        // The figures and the sections first, the chapters' last pages then (as those left them), then the figures and
        // the sections of the chapters set tighter, again.
        let changed = match step {
            Step::Arrange => {
                arrange(&places, &page.leads, &mut layout) || {
                    step = Step::Fit;
                    refit(&places, &mut layout.fits)
                }
            }
            Step::Fit => {
                refit(&places, &mut layout.fits) || {
                    step = Step::Rearrange;
                    let tighter: BTreeSet<u32> = layout
                        .fits
                        .iter()
                        .filter(|(_, fit)| fit.level > 0)
                        .filter_map(|(chapter, _)| chapter.split('-').next()?.parse().ok())
                        .collect();
                    unarrange(&mut layout, &tighter, &page.leads)
                        || arrange(&places, &page.leads, &mut layout)
                }
            }
            Step::Rearrange => arrange(&places, &page.leads, &mut layout),
        };
        written = Some(page);
        let found: BTreeMap<String, u32> = places
            .into_iter()
            .filter(|(name, _)| !name.starts_with(END) && !name.starts_with(FIGURE))
            .map(|(name, place)| (name, place.page))
            .collect();
        if !changed && found == pages {
            break;
        }
        pages = found;
    }
    let tighter = layout.fits.values().filter(|fit| fit.level > 0).count();
    let (smaller, new_pages) = written
        .as_ref()
        .map_or((0, 0), |page| (page.smaller, page.new_pages));
    println!(
        "manual ({lang}): {} ({tighter} chapters set tighter, {smaller} figures made smaller to stay on their page, \
         {new_pages} sections moved to the next page with their figure)",
        target.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::arrange;
    use super::{
        END, Fit, Layout, Place, Shrink, TEXT_BOTTOM, TEXT_TOP, anchor, callouts, chapter_sections,
        depth, destinations, figures, glossary, index, is_chapter, join_japanese_lines,
        link_number, links, misnumbered, number_figures, pdf_name, private_paths, refit, region,
        release_version, relink, results, section_leads, unarrange, with_pages,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    #[test]
    fn figures_are_numbered_in_their_chapter_and_results_marked() {
        let html = "<h1 id=\"a\">3. A</h1><figure><img/><figcaption>x</figcaption></figure>\
                    <figure><img/><figcaption>y</figcaption></figure><h1 id=\"b\">4. B</h1>\
                    <figure><img/><figcaption>z</figcaption></figure>";
        let made = number_figures(html, "図");
        assert!(made.contains("<figcaption><span class=\"fignum\">図 3-1</span>x</figcaption>"));
        assert!(made.contains("<figcaption><span class=\"fignum\">図 3-2</span>y</figcaption>"));
        assert!(made.contains("<figcaption><span class=\"fignum\">図 4-1</span>z</figcaption>"));
        assert_eq!(
            results(
                "<p><strong>結果</strong>: できた</p><p><strong>名前</strong></p>",
                "結果"
            ),
            "<p class=\"result\"><strong>結果</strong>: できた</p><p><strong>名前</strong></p>"
        );
    }

    /// Figures under headings, in steps, and after paragraphs.
    const SAMPLE: &str = "<h1 id=\"a\">3. A</h1><h2 id=\"s\">3.1 S</h2>\n<p>intro</p>\n\
                    <figure><img/><figcaption>x</figcaption></figure>\
                    <ol><li><p>step</p>\n<figure><img/><figcaption>y</figcaption></figure></li></ol>\
                    <h3 id=\"t\">T</h3><figure><img/></figure>\
                    <h2 id=\"u\">3.2 U</h2>\n<ol start=\"2\">\n<li>\n<p>first</p>\n<figure><img/></figure></li></ol>\
                    <h2 id=\"v\">3.3 V</h2>\n<p>lead</p>\n<ul>\n<li>\n<p>one</p>\n<figure><img/></figure></li></ul>\
                    <p>a</p>\n<p>b</p>\n<figure><img/></figure>\
                    <h3 id=\"w\">W</h3>\n<p>c</p>\n<p>d</p>\n<figure><img/></figure>";

    #[test]
    fn figures_are_marked_with_what_goes_with_them() {
        // Each figure is named and marked: under its picture, at its end, and before what goes with it.
        let made = number_figures(SAMPLE, "図");
        assert!(made.contains("<div class=\"mark\" id=\"fig-3-1-b2\"></div><h2 id=\"s\">"));
        assert!(made.contains(
            "<figure id=\"fig-3-1\"><img/><div class=\"mark\" id=\"fig-3-1-i\"></div>\
             <figcaption><span class=\"fignum\">図 3-1</span>x</figcaption>\
             <div class=\"mark\" id=\"fig-3-1-e\"></div></figure>"
        ));
        // A step's paragraph goes with its figure; so does the heading before the list when it is the first step.
        assert!(made.contains(
            "</figure><div class=\"mark\" id=\"fig-3-2-b\"></div><ol><li><p>step</p>\n<figure id=\"fig-3-2\">"
        ));
        assert!(made.contains(
            "<div class=\"mark\" id=\"fig-3-3-b3\"></div><h3 id=\"t\">T</h3><figure id=\"fig-3-3\"><img/>\
             <div class=\"mark\" id=\"fig-3-3-i\"></div><div class=\"mark\" id=\"fig-3-3-e\"></div></figure>"
        ));
        assert!(made.contains(
            "</figure><div class=\"mark\" id=\"fig-3-4-b2\"></div><h2 id=\"u\">3.2 U</h2>\n<ol start=\"2\">\n<li>\n\
             <p>first</p>\n<figure id=\"fig-3-4\">"
        ));
        // A paragraph that leads to the list goes too; one before another paragraph does not.
        assert!(made.contains("<div class=\"mark\" id=\"fig-3-5-b2\"></div><h2 id=\"v\">"));
        assert!(made.contains("<p>a</p>\n<div class=\"mark\" id=\"fig-3-6-b\"></div><p>b</p>"));
        // Unless the paragraph before is the first under a heading.
        assert!(
            made.contains(
                "<div class=\"mark\" id=\"fig-3-7-b3\"></div><h3 id=\"w\">W</h3>\n<p>c</p>"
            )
        );
    }

    #[test]
    fn a_figure_sent_on_for_a_little_room_is_made_smaller() {
        let made = number_figures(SAMPLE, "図");
        // A figure 250 points high sent to page 6 with 250 points left on page 5 under its heading: it needs 268.
        let figure = |places: &mut BTreeMap<String, Place>, id: &str, kept: &str, room: f32| {
            let top = TEXT_TOP;
            places.insert(
                format!("{id}-{kept}"),
                Place {
                    page: 5,
                    y: TEXT_BOTTOM + room,
                },
            );
            places.insert(id.to_string(), Place { page: 6, y: top });
            places.insert(
                format!("{id}-i"),
                Place {
                    page: 6,
                    y: top - 250.0,
                },
            );
            places.insert(
                format!("{id}-e"),
                Place {
                    page: 6,
                    y: top - 268.0,
                },
            );
        };
        let mut places = BTreeMap::new();
        figure(&mut places, "fig-3-1", "b2", 250.0 + 17.0);
        figure(&mut places, "fig-3-2", "b", 260.0);
        figure(&mut places, "fig-4-1", "b", 120.0);
        let mut layout = Layout::default();
        assert!(arrange(&places, &[], &mut layout));
        let first = layout.shrinks.get("fig-3-1").copied().unwrap_or_default();
        assert!((first.original - 250.0).abs() < 0.01);
        assert_eq!((first.tries, first.settled), (1, false));
        assert!(
            first
                .height
                .is_some_and(|height| (height - (250.0 - 18.0 - 2.0 - 1.5)).abs() < 0.01)
        );
        // The chapter's later figure waits; one that would have to lose half its height stays as it is.
        assert!(!layout.shrinks.contains_key("fig-3-2"));
        assert_eq!(
            layout
                .shrinks
                .get("fig-4-1")
                .map(|shrink| (shrink.height, shrink.settled)),
            Some((None, true))
        );
        // Back on its page (200 points high there), the first stays; the next lacks 9 points (11 with the slack), which
        // it shares with the first, on the page before it.
        places.insert("fig-3-1".to_string(), Place { page: 5, y: 300.0 });
        for mark in ["i", "e"] {
            places.insert(format!("fig-3-1-{mark}"), Place { page: 5, y: 100.0 });
        }
        assert!(arrange(&places, &[], &mut layout));
        let scale = 1.0 - 11.0 / 450.0;
        let held = |layout: &Layout, id: &str| {
            layout
                .shrinks
                .get(id)
                .and_then(|shrink| shrink.height)
                .unwrap_or_default()
        };
        assert!((held(&layout, "fig-3-1") - (200.0 * scale - 1.5)).abs() < 0.01);
        assert!((held(&layout, "fig-3-2") - (250.0 * scale - 1.5)).abs() < 0.01);
        assert!(
            layout
                .shrinks
                .get("fig-3-1")
                .is_some_and(|shrink| shrink.settled)
        );
        // Not brought back by that height, nor asked to be smaller still: set back, and the first with it.
        assert!(arrange(&places, &[], &mut layout));
        assert!(!arrange(&places, &[], &mut layout));
        assert_eq!(
            layout.shrinks.get("fig-3-2").map(|shrink| (
                shrink.height,
                shrink.tries,
                shrink.settled
            )),
            Some((None, 1, true))
        );
        assert!((held(&layout, "fig-3-1") - 228.5).abs() < 0.01);
        // Held figures are linked from the hidden list and held; sections start a new page.
        layout.shrinks.insert(
            "fig-3-3".to_string(),
            Shrink {
                original: 250.0,
                height: Some(230.0),
                tries: 1,
                settled: true,
            },
        );
        layout.breaks.insert("u".to_string());
        let (sections, css) = chapter_sections(&made, "Book", &layout);
        assert!(sections.contains(
            "<a href=\"#fig-3-1-b2\"></a><a href=\"#fig-3-1\"></a><a href=\"#fig-3-1-i\"></a>"
        ));
        assert!(css.contains("#fig-3-1 img { max-height: 228.5pt; }"));
        assert!(css.contains("#fig-3-3 img { max-height: 230.0pt; }"));
        assert!(sections.contains("<h2 id=\"u\" class=\"new-page\">3.2 U</h2>"));
        layout.fits.insert(
            "a".to_string(),
            Fit {
                level: 1,
                pages: 1,
                settled: true,
            },
        );
        // In a chapter set tight, no higher than its pictures may be.
        let (sections, css) = chapter_sections(&made, "Book", &layout);
        assert!(css.contains("#fig-3-1 img { max-height: 215.4pt; }"));
        assert!(css.contains("#fig-3-3 img { max-height: 215.4pt; }"));
        assert!(sections.contains("class=\"new-page\""));
    }

    #[test]
    fn a_section_left_low_on_a_page_without_its_figure_starts_the_next() {
        let html = "<h1 id=\"c\">3. C</h1><h2 id=\"s\">3.1 S</h2><p>a</p>\
                    <figure id=\"fig-3-1\"></figure><figure id=\"fig-3-2\"></figure>\
                    <h2 id=\"t\">3.2 T</h2><p>b</p><h1 id=\"d\">4. D</h1><figure id=\"fig-4-1\"></figure>\
                    <h2 id=\"u\">4.1 U</h2><figure id=\"fig-4-2\"></figure>";
        assert_eq!(
            section_leads(html),
            vec![
                ("s".to_string(), "fig-3-1".to_string()),
                ("u".to_string(), "fig-4-2".to_string()),
            ]
        );
        // Section s: its heading in the last fifth of page 4, its figure on page 5, 0.93 of a page in all. Section u:
        // its heading halfway down page 8, 0.8 of a page.
        let place = |page, depth_wanted: f32| Place {
            page,
            y: TEXT_TOP - depth_wanted * (TEXT_TOP - TEXT_BOTTOM),
        };
        let mut places: BTreeMap<String, Place> = [
            ("s".to_string(), place(4, 0.82)),
            ("fig-3-1".to_string(), place(5, 0.4)),
            ("fig-3-1-i".to_string(), place(5, 0.7)),
            ("fig-3-1-e".to_string(), place(5, 0.75)),
            ("u".to_string(), place(8, 0.5)),
            ("fig-4-2".to_string(), place(9, 0.0)),
            ("fig-4-2-i".to_string(), place(9, 0.25)),
            ("fig-4-2-e".to_string(), place(9, 0.3)),
        ]
        .into();
        let leads = section_leads(html);
        let mut layout = Layout::default();
        assert!(arrange(&places, &leads, &mut layout));
        assert_eq!(layout.breaks, ["s".to_string()].into());
        assert!(
            layout
                .shrinks
                .get("fig-3-1")
                .is_some_and(|shrink| shrink.settled)
        );
        assert!(!arrange(&places, &leads, &mut layout));
        let moves = |places: &BTreeMap<String, Place>| {
            let mut layout = Layout::default();
            arrange(places, &leads, &mut layout);
            layout.breaks.contains("u")
        };
        // Not when the heading and the figure would not fit on one page either.
        places.insert("u".to_string(), place(8, 0.75));
        places.insert("fig-4-2-e".to_string(), place(9, 0.9));
        assert!(!moves(&places));
        // Higher on its page, only a short section.
        places.insert("u".to_string(), place(8, 0.6));
        places.insert("fig-4-2-e".to_string(), place(9, 0.5));
        assert!(!moves(&places));
        places.insert("fig-4-2-e".to_string(), place(9, 0.35));
        assert!(moves(&places));
        // A chapter set tighter is fitted again from nothing.
        let mut layout = Layout::default();
        arrange(&places, &leads, &mut layout);
        layout
            .shared
            .insert("fig-3-1".to_string(), vec![("fig-3-9".to_string(), None)]);
        assert!(unarrange(&mut layout, &BTreeSet::from([3]), &leads));
        assert_eq!(layout.breaks, ["u".to_string()].into());
        assert!(
            layout
                .shrinks
                .keys()
                .all(|figure| figure.starts_with("fig-4-"))
        );
        assert!(layout.shared.is_empty());
        assert!(!unarrange(&mut layout, &BTreeSet::from([3]), &leads));
    }

    #[test]
    fn a_release_names_its_manuals_and_keeps_this_computer_out() {
        assert_eq!(release_version("v0.1.0-alpha.15"), Some("0.1.0-alpha.15"));
        assert_eq!(release_version("0.2.0"), Some("0.2.0"));
        assert_eq!(release_version("vnext"), None);
        assert_eq!(release_version("v1.0/../x"), None);
        let private = private_paths(Path::new(r"C:\work\DiscCraft"));
        assert!(private.contains(&r"C:\work\DiscCraft".to_owned()));
        assert!(private.contains(&"C:/work/DiscCraft".to_owned()));
        assert!(private.contains(&"file:/".to_owned()));
    }

    #[test]
    fn a_link_named_by_a_number_goes_to_that_number() {
        assert_eq!(link_number("5.1"), Some((5, Some(1))));
        assert_eq!(link_number("5.1 読み込み方"), Some((5, Some(1))));
        assert_eq!(link_number("14 章"), Some((14, None)));
        assert_eq!(link_number("Chapter 14"), Some((14, None)));
        assert_eq!(link_number("Chapter 3 shows"), Some((3, None)));
        assert_eq!(link_number("Chapters"), None);
        assert_eq!(link_number("3. はじめての DVD を作る"), Some((3, None)));
        assert_eq!(link_number("17.10 まだないもの"), Some((17, Some(10))));
        assert_eq!(link_number("1080i / 1080p / 720p"), None);
        assert_eq!(link_number("16:9 / 4:3"), None);
        assert_eq!(link_number("H.264"), None);
        let text = "[5.1 読み込み方](05-assets.md#51-読み込み方) [14.7](#177-x) [13](14-build.md#14-ビルドする)
                    [12 章](12-check.md#12-x) ![3.1](../images/ja/a.png) [24.1 索引](#241-索引) [README](README.md)
                    ```
[9](01-about.md)
```
";
        let found = misnumbered("17-advanced.md", text);
        assert_eq!(
            found,
            vec![
                (1, "[14.7](#177-x) does not go to that number".to_string()),
                (
                    1,
                    "[13](14-build.md#14-ビルドする) does not go to that number".to_string()
                ),
                (
                    2,
                    "[24.1 索引](#241-索引) does not go to that number".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_short_last_page_is_set_tighter_only_when_that_saves_it() {
        let place = |page, depth_wanted: f32| Place {
            page,
            y: 841.92 - depth_wanted * 734.2,
        };
        let mut fits = BTreeMap::new();
        // Chapter a: three lines on its third page. Chapter b: one page. Chapter c: half its second page.
        let mut places: BTreeMap<String, Place> = [
            ("a".to_string(), place(1, 0.0)),
            (format!("{END}a"), place(3, 0.05)),
            ("b".to_string(), place(4, 0.0)),
            (format!("{END}b"), place(4, 0.9)),
            ("c".to_string(), place(5, 0.0)),
            (format!("{END}c"), place(6, 0.5)),
        ]
        .into();
        assert!(refit(&places, &mut fits));
        assert_eq!(
            fits.get("a"),
            Some(&Fit {
                level: 1,
                pages: 2,
                settled: false
            })
        );
        assert!(
            fits.get("b")
                .is_some_and(|fit| fit.settled && fit.level == 0)
        );
        assert!(
            fits.get("c")
                .is_some_and(|fit| fit.settled && fit.level == 0)
        );
        // Tight does not save the page, tighter does: kept tighter.
        assert!(refit(&places, &mut fits));
        assert_eq!(fits.get("a").map(|fit| fit.level), Some(2));
        places.insert(format!("{END}a"), place(2, 0.97));
        assert!(!refit(&places, &mut fits));
        assert_eq!(
            fits.get("a"),
            Some(&Fit {
                level: 2,
                pages: 2,
                settled: true
            })
        );
        // A chapter no level saves goes back as it was.
        let mut fits = BTreeMap::new();
        places.insert(format!("{END}a"), place(3, 0.05));
        for _ in 0..3 {
            refit(&places, &mut fits);
        }
        assert_eq!(
            fits.get("a"),
            Some(&Fit {
                level: 0,
                pages: 2,
                settled: true
            })
        );
        // The browser's heights leave the top margin out: the text runs from the page's top down 734 points.
        assert!((depth(841.92) - 0.0).abs() < 1e-3 && (depth(107.72) - 1.0).abs() < 1e-3);
        // The chapters become sections with their level and an end marker linked from a hidden list.
        let fits: BTreeMap<String, Fit> = [(
            "y".to_string(),
            Fit {
                level: 2,
                pages: 1,
                settled: true,
            },
        )]
        .into();
        let (made, css) = chapter_sections(
            "<h1 id=\"x\">X &amp; <code>\"x\"</code></h1>\n<p>a</p>\n<h1 id=\"y\">Y</h1>\n",
            "Book",
            &Layout {
                fits,
                ..Layout::default()
            },
        );
        assert!(made.starts_with("<section class=\"chapter\" style=\"page: chapter0\"><h1 id=\"x\">X &amp; <code>\"x\"</code></h1>\n<p>a</p>\n<div class=\"end\" id=\"end-x\"></div></section>"));
        assert!(
            made.contains(
                "<section class=\"chapter tighter\" style=\"page: chapter1\"><h1 id=\"y\">"
            )
        );
        assert!(
            made.ends_with(
                "<nav class=\"ends\"><a href=\"#end-x\"></a><a href=\"#end-y\"></a></nav>"
            )
        );
        // Each chapter's pages carry the book's title and the chapter's at their head.
        assert!(css.contains(
            "@page chapter0 { @top-left { content: \"Book\"; } @top-right { content: \"X & \\\"x\\\"\"; } }"
        ));
        assert!(css.contains("@top-right { content: \"Y\"; }"));
        // A list of where to read: each link followed by its page.
        let pages: BTreeMap<String, u32> = [("a".to_string(), 12)].into();
        assert_eq!(
            with_pages(
                "<td><a href=\"#a\">A</a>、<a href=\"#z\">Z</a></td>",
                &pages
            ),
            "<td><a href=\"#a\">A</a><span class=\"pageref\">12</span>、<a href=\"#z\">Z</a></td>"
        );
    }

    #[test]
    fn the_glossary_gets_its_pages() {
        let pages: BTreeMap<String, u32> = [("a-は".to_string(), 7), ("b".to_string(), 9)].into();
        // The index: a paragraph a group, made a list with leaders and pages (the href percent-encoded or not).
        let html = "x<!-- index -->
<p><strong>あ行</strong> <a href=\"#a-%E3%81%AF\">は</a> ・ <a href=\"#b\">b</a></p>
<!-- /index -->y";
        let made = region(html, "index", |part| index(part, &pages));
        assert!(made.starts_with(
            "x<div class=\"index\"><div class=\"group\"><p class=\"label\">あ行</p><a href=\"#a-%E3%81%AF\">"
        ));
        assert!(made.contains(
            "<span class=\"words\">は</span><span class=\"dots\"></span><span class=\"page\">7</span>"
        ));
        assert!(made.contains("<span class=\"words\">b</span><span class=\"dots\"></span><span class=\"page\">9</span>"));
        assert!(made.ends_with("</a></div></div>y"));
        // The terms: each group in its columns, each term kept together, its `→` line's links given their pages.
        let html = "<h2 id=\"g\">G</h2>
<h3 id=\"a\">A</h3>
<p>words</p>
<p>→ <a href=\"#b\">b</a> ・ <a href=\"#c\">c</a></p>
<h3 id=\"b\">B</h3>
<p>more</p>
";
        let made = glossary(html, &pages);
        assert!(
            made.starts_with("<div class=\"glossary\"><h2 id=\"g\">G</h2><div class=\"terms\">")
        );
        assert_eq!(made.matches("<div class=\"term\">").count(), 2);
        assert!(made.contains(
            "<p class=\"see\">→ <a href=\"#b\">b</a><span class=\"pageref\">9</span> ・ <a href=\"#c\">c</a></p>"
        ));
        assert!(made.contains("<p>words</p>"));
        assert!(made.ends_with(
            "<p>more</p>
</div></div></div>"
        ));
        // No markers: nothing changes.
        assert_eq!(region("<p>a</p>", "index", |_| String::new()), "<p>a</p>");
    }

    #[test]
    fn anchors_are_githubs() {
        assert_eq!(
            anchor("3. はじめての DVD を作る"),
            "3-はじめての-dvd-を作る"
        );
        assert_eq!(
            anchor("21. AI エージェントから使う（MCP）"),
            "21-ai-エージェントから使うmcp"
        );
        assert_eq!(
            anchor("17.7 ボタンへのリンクの入れ方 5 とおり"),
            "177-ボタンへのリンクの入れ方-5-とおり"
        );
        assert!(is_chapter("03-tutorial.md"));
        assert!(!is_chapter("README.md"));
    }

    #[test]
    fn links_become_the_books() {
        let mut first = BTreeMap::new();
        first.insert("15-labels.md", "15-ディスクのレーベルを作る".to_owned());
        assert_eq!(
            relink(
                "見る: [15 章](15-labels.md#153-紐づけ) と [表](../../ui-parity.md)。",
                &first,
                "ja"
            ),
            "見る: [15 章](#153-紐づけ) と 表。"
        );
        assert_eq!(
            relink("[15 章](15-labels.md)", &first, "ja"),
            "[15 章](#15-ディスクのレーベルを作る)"
        );
        assert_eq!(
            relink("![画面](../images/ja/screen/window.png)", &first, "ja"),
            "![画面](images/screen/window.png)"
        );
        assert_eq!(
            figures(
                "<p><img src=\"a.png\" alt=\"画面\" /></p><p>text <img src=\"b.png\" alt=\"\" /></p>"
            ),
            "<figure><img src=\"a.png\" alt=\"画面\" /><figcaption>画面</figcaption></figure><p>text <img src=\"b.png\" alt=\"\" /></p>"
        );
        assert_eq!(
            callouts(
                "<blockquote>
<p><strong>詳しく</strong>: x</p></blockquote><blockquote><p>y</p></blockquote>"
            ),
            "<blockquote class=\"details\">
<p><strong>詳しく</strong>: x</p></blockquote><blockquote><p>y</p></blockquote>"
        );
        let mut html = String::new();
        let events = pulldown_cmark::Parser::new(
            "日本語の
文と DVD の
Disc
words",
        )
        .collect();
        pulldown_cmark::html::push_html(&mut html, join_japanese_lines(events).into_iter());
        assert_eq!(
            html,
            "<p>日本語の文と DVD の
Disc
words</p>
"
        );
        assert_eq!(pdf_name(b"3-#25E3#2581#25AF-dvd"), "3-は-dvd");
        let pdf = b"%PDF
1 0 obj
<</Type /Pages
/Kids [2 0 R 3 0 R]>>
endobj
2 0 obj
<</Type /Page>>
endobj
3 0 obj
<</Type /Page>>
endobj
4 0 obj
<</a-#25E3#2581#25AF [3 0 R /XYZ 0 800 0]
/b [2 0 R /XYZ 0 100 0]>>
endobj
5 0 obj
<</Type /Catalog
/Pages 1 0 R
/Dests 4 0 R>>
endobj
";
        let places = destinations(pdf);
        assert_eq!(places.get("a-は"), Some(&Place { page: 2, y: 800.0 }));
        assert_eq!(places.get("b"), Some(&Place { page: 1, y: 100.0 }));
        let found = links("[a](b.md) ![c](../images/ja/d.png)\n```\n[x](y.md)\n```\n");
        assert_eq!(found.len(), 2);
        assert!(!found[0].2 && found[1].2);
    }
}
