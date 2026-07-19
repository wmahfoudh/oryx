//! Icon pipeline: rasterizes the oryx mark at the standard icon sizes
//! into OUT_DIR as raw RGBA plus a multi-size ICO. Windows builds embed
//! the ICO as the executable's icon resource when a resource compiler is
//! available; its absence only skips the exe icon, never the build.

use std::path::{Path, PathBuf};
use std::process::Command;

const SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/oryx.svg");
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
        embed_exe_icon(&out, &ico_path);
    }
}

/// Compiles a resource script referencing the ICO and links it into the
/// executable. Skips with a note when no windres is on the path.
fn embed_exe_icon(out: &Path, ico_path: &Path) {
    let windres = ["x86_64-w64-mingw32-windres", "windres"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        });
    let Some(windres) = windres else {
        println!("cargo:warning=windres not found, exe icon resource skipped");
        return;
    };
    let rc = out.join("oryx.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico_path.display())).expect("write rc");
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
