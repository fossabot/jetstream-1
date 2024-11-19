#[cfg(feature = "docs")]
use comrak::{
    format_html,
    nodes::{Ast, AstNode, NodeValue},
    parse_document, Arena, Options,
};
use std::{
    fs::{self, create_dir_all, read_dir, File},
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
};
const SOURCE: &str = "docs/SUMMARY.md";

fn main() -> anyhow::Result<()> {
    let cargo_docs_feature = std::env::var("CARGO_FEATURE_DOCS");
    if cargo_docs_feature.is_err() {
        return Ok(());
    }
    let target_dir = "target";
    let docs_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(target_dir)
        .join("doc");
    glob::glob(
        format!("{}/**/*.html", docs_path.as_path().to_string_lossy()).as_str(),
    )
    .expect("Failed to read glob pattern")
    .for_each(|entry| {
        let entry = entry.unwrap();
        if entry.to_str().unwrap().contains("doc/jetstream") {
            println!("cargo:rerun-if-changed={}", entry.to_str().unwrap());
        }
        println!("cargo:rerun-if-changed={}", entry.to_str().unwrap());
    });
    #[cfg(not(feature = "docs"))]
    return Ok(());
    #[cfg(feature = "docs")]
    try_make_books();
}
#[cfg(feature = "docs")]
fn try_make_books() -> anyhow::Result<()> {
    // open index.html from target/doc/*/index.html
    // CARGO_BUILD_TARGET_DIR
    let target_dir = "target";
    let docs_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(target_dir)
        .join("doc");

    let mut org_docs = ["0intro.md"].map(|s| PathBuf::from(s)).to_vec();

    let mut docs: Vec<_> = glob::glob(
        format!("{}/**/*.html", docs_path.as_path().to_string_lossy()).as_str(),
    )
    .expect("Failed to read glob pattern")
    .map(|entry| entry.unwrap())
    .collect();
    docs.sort_unstable();
    let mut docs = docs.iter().flat_map(|entry| {
        let full_path = entry.as_path();
        let last = full_path.with_extension("md");

        if last.ends_with("all.md")
        // || full_path.ends_with("index.md")
        {
            return None;
        }

        // make it relative to docs_path
        let last = last.strip_prefix(docs_path.clone()).unwrap();

        let base_name: Vec<_> =
            last.components().map(|c| c.as_os_str()).collect();
        let base_name = base_name[..base_name.len() - 1]
            .into_iter()
            .flat_map(|c| c.to_str())
            .collect::<Vec<&str>>()
            .join("/");

        if !base_name.as_str().starts_with("jetstream") {
            return None;
        }
        let outpath = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("docs/crates/")
            .join(last);

        let target_base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("docs/crates/")
            .join(base_name.clone());

        // if basename doesn't exist mkdir -p it into existence
        if read_dir(target_base_path.clone()).is_err() {
            create_dir_all(target_base_path.clone()).expect("mkdir -p");
        }

        let html = File::open(full_path)
            .expect(format!("index.html not found: {:?}", full_path).as_str());

        let reso = match indexed_docs::convert_rustdoc_to_markdown(html) {
            Ok(md) => md,
            Err(e) => {
                eprintln!("Error converting rustdoc to markdown: {:?}", e);
                return None;
            }
        };

        let mut md = reso.0;
        // lets do some simple replacements.
        // we should have here a set of transformers that are fn(&mut str)
        for item in reso.1 {
            let item_quoted = format!(r#"`{}`"#, item.name);

            let linked = format!("[{}]({})", item_quoted, item.url_path());
            dbg!(&item_quoted, &linked);
            eprintln!("replacing {} with {}", &item_quoted, &linked);
            md = md.replace(&item_quoted, &linked);
        }

        std::fs::write(outpath.clone(), md).expect("book");

        let rel_path: PathBuf = outpath
            .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/"))
            .unwrap()
            .into();
        Some(rel_path)
    });
    let mut docs = docs.collect::<Vec<_>>();
    docs.sort_unstable();
    docs.sort_by(|a, b| {
        // strip index.md from the end for both a and b
        let a = a
            .to_str()
            .unwrap_or("")
            .strip_suffix("index.md")
            .unwrap_or(a.to_str().unwrap());
        let b = b
            .to_str()
            .unwrap_or("")
            .strip_suffix("index.md")
            .unwrap_or(b.to_str().unwrap());
        a.cmp(b)
    });

    // org_docs.append(&mut docs);
    // let mut docs = org_docs;
    // docs.sort_unstable();
    let mut file = File::create("docs/SUMMARY.md")?;

    let write_docs = |docs: Vec<PathBuf>,
                      mut file: File,
                      indent: usize|
     -> Option<File> {
        let docs = docs.into_iter().map(|doc| PathBuf::from(doc));

        for doc in docs {
            let components = doc.components();
            let count = components.clone().count();
            // first print components.len() - 1 tabs
            // then print the last component
            let a = components.collect::<Vec<_>>();
            let a = a[0..count].iter().map(|c| c.as_os_str().to_str().unwrap());
            let name = a.map(|f| f.to_string()).collect::<Vec<String>>();

            let full_path = env!("CARGO_MANIFEST_DIR").to_string()
                + "/docs/"
                + doc.to_str().unwrap();

            let mut title = get_title(full_path.as_str());
            if !full_path.ends_with("playground/index.md") && title.is_empty() {
                println!("title is empty for: {:?}", full_path);
                continue;
            }
            if full_path.ends_with("playground/index.md") {
                title = "&#xec2b; srclang ide".to_string();
            }

            for c in name.iter().enumerate() {
                let header_level = c.0;
                for _ in 3..header_level {
                    // write!(file, "\t")?;
                }
                if c.0 == name.len() - 1 {
                    break;
                }
                // write!(
                //     file,
                //     "- [{}](#?{})\n",
                //     title[count - 1],
                //     title[0..count - 1].join("/")
                // )?;
            }
            let title_splat = title.split(" ");
            let firstbit = title_splat.clone().next().unwrap();
            let lastbit = title_splat.clone().last().unwrap();
            let lastbit_splat = lastbit.split("::");
            let lastbit_count = lastbit_splat.clone().count();
            for _ in 0..lastbit_count - 1 + indent {
                write!(file, "\t").unwrap_or(())
            }
            let ti = if lastbit_count == 1 {
                title.clone()
            } else {
                lastbit_splat.last().unwrap().to_string()
            };
            let icon = match firstbit {
                "Struct" => "&#xea91;",
                "Enum" => "&#xea95;",
                "Trait" => "&#xeb61;",
                "Function" => "&#xea8c;",
                "Type" => "&#xea92;",
                "Macro" => "&#xeb66;",
                "Constant" => "&#xeb5d;",
                "Module" => "&#xea8b;",
                "Crate" => "&#xeb29;",
                "Crates" => "🦀",
                _ => "",
            };
            writeln!(file, "- [{} {}]({})", icon, ti, doc.to_str().unwrap())
                .unwrap_or(());
        }
        Some(file)
    };

    write_docs(docs, write_docs(org_docs, file, 0).unwrap(), 1).unwrap();
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemPath {
    components: Vec<String>,
}
#[cfg(feature = "docs")]
fn parsed_title(path: &str) -> Option<ItemPath> {
    let title = get_title(path);
    let title = title.split(" ").last().unwrap().split("::");
    let components: Vec<_> = title.map(|c| c.to_string()).collect();
    // filter out empty strings
    let components: Vec<_> =
        components.into_iter().filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return None;
    }
    Some(ItemPath { components })
}
#[cfg(feature = "docs")]
fn parse_title(contents: &str) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, &contents, &Options::default());
    let mut title = String::new();
    for node in root.descendants() {
        if let NodeValue::Heading(_) = node.data.borrow().value {
            let mut txt_buf = String::new();
            for text_node in node.descendants() {
                if let NodeValue::Text(ref text) = text_node.data.borrow().value
                {
                    txt_buf.push_str(text);
                }
            }
            title = txt_buf;
            break;
        }
    }
    title
}
#[cfg(feature = "docs")]
fn get_title(path: &str) -> String {
    let file =
        File::open(path).expect(format!("file not found: {:?}", path).as_str());
    // read all the file
    let mut reader = BufReader::new(file);
    let mut contents = String::new();
    reader.read_to_string(&mut contents).expect("read failed");
    parse_title(&contents)
}
