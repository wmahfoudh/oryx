//! Icon pipeline: rasterizes the oryx mark at the standard icon sizes
//! into OUT_DIR as raw RGBA plus a multi-size ICO. Windows builds embed
//! the ICO and a version block naming the app as the executable's
//! resources when a resource compiler is available; its absence only
//! skips the resources, never the build.
//!
//! Grammar pipeline: compiles the `.sublime-syntax` sources under
//! `assets/syntaxes/` together with syntect's default set into one
//! serialized dump in OUT_DIR, which the highlighter embeds.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The resource script content, shared with the library so its tests
/// cover what gets compiled into the executable.
mod resource {
    include!("src/platform/resource.rs");
}

const SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/oryx.svg");
    println!("cargo:rerun-if-changed=src/platform/resource.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let svg = std::fs::read("assets/icon/oryx.svg").expect("icon svg readable");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&svg, &options).expect("icon svg parses");
    let intrinsic = tree.size().width().max(tree.size().height());

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in SIZES {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("icon pixmap allocation");
        let scale = size as f32 / intrinsic;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let rgba: Vec<u8> = pixmap
            .pixels()
            .iter()
            .flat_map(|px| {
                let c = px.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        std::fs::write(out.join(format!("icon_{size}.rgba")), &rgba).expect("write rgba");
        let image = ico::IconImage::from_rgba_data(size, size, rgba);
        dir.add_entry(ico::IconDirEntry::encode(&image).expect("ico entry"));
    }
    let ico_path = out.join("oryx.ico");
    let file = std::fs::File::create(&ico_path).expect("create ico");
    dir.write(file).expect("write ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_exe_resources(&out, &ico_path);
    }

    build_syntax_dump(&out);
}

/// One dump holds the default set plus the bundled grammar sources, so
/// the highlighter loads everything in a single deserialization.
fn build_syntax_dump(out: &Path) {
    watch_tree(Path::new("assets/syntaxes"));
    let mut builder = syntect::parsing::SyntaxSet::load_defaults_newlines().into_builder();
    builder
        .add_from_folder("assets/syntaxes", true)
        .expect("bundled grammars compile");
    syntect::dumps::dump_to_file(&builder.build(), out.join("syntaxes.packdump"))
        .expect("write syntax dump");
}

/// Cargo tracks a directory's own mtime only, so every file below the
/// grammar tree is named individually.
fn watch_tree(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in std::fs::read_dir(dir).expect("readable syntax dir") {
        let path = entry.expect("syntax dir entry").path();
        if path.is_dir() {
            watch_tree(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Compiles the resource script (icon and version block) and links it
/// into the executable. Skips with a note when no windres is on the path.
fn embed_exe_resources(out: &Path, ico_path: &Path) {
    let windres = ["x86_64-w64-mingw32-windres", "windres"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        });
    let Some(windres) = windres else {
        println!("cargo:warning=windres not found, exe resources skipped");
        return;
    };
    let version_digits = format!(
        "{},{},{},0",
        std::env::var("CARGO_PKG_VERSION_MAJOR").expect("version major"),
        std::env::var("CARGO_PKG_VERSION_MINOR").expect("version minor"),
        std::env::var("CARGO_PKG_VERSION_PATCH").expect("version patch"),
    );
    let version = std::env::var("CARGO_PKG_VERSION").expect("version");
    let rc = out.join("oryx.rc");
    std::fs::write(
        &rc,
        resource::resource_script(&ico_path.display().to_string(), &version_digits, &version),
    )
    .expect("write rc");
    let res = out.join("oryx.res");
    let status = Command::new(windres)
        .arg(&rc)
        .args(["-O", "coff", "-o"])
        .arg(&res)
        .status()
        .expect("run windres");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", res.display());
}
