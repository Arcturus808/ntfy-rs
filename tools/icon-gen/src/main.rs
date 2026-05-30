use std::fs;
use std::io::{BufWriter, Write};

fn main() {
    let svg_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: icon-gen <input.svg> <output-dir>");
        std::process::exit(1);
    });
    let out_dir = std::env::args().nth(2).unwrap_or_else(|| "assets".to_string());

    let svg_data = fs::read(&svg_path).expect("failed to read SVG");

    // Load system fonts so <text> elements render correctly (e.g. Arial on Windows).
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let opts = resvg::usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&svg_data, &opts).expect("failed to parse SVG");

    let sizes = [16, 32, 48, 180, 256];
    let mut ico_pngs: Vec<(u32, Vec<u8>)> = Vec::new();

    fs::create_dir_all(&out_dir).ok();

    for &size in &sizes {
        let pixmap = render(&tree, size);
        let png_bytes = encode_png(&pixmap, size);

        // Collect sizes for ICO (16, 32, 48, 256)
        if [16, 32, 48, 256].contains(&size) {
            ico_pngs.push((size, png_bytes.clone()));
        }

        // Save individual PNGs (skip sizes only needed inside ICO)
        let name = match size {
            16 => "favicon-16.png",
            32 => "favicon-32.png",
            48 => continue,
            180 => "apple-touch-icon.png",
            256 => continue,
            _ => continue,
        };
        let path = format!("{out_dir}/{name}");
        fs::write(&path, &png_bytes).expect("failed to write PNG");
        println!("  wrote {path} ({size}x{size})");
    }

    // Write favicon.ico
    let ico_path = format!("{out_dir}/favicon.ico");
    let ico_sizes: Vec<u32> = ico_pngs.iter().map(|(s, _)| *s).collect();
    let ico_data: Vec<Vec<u8>> = ico_pngs.iter().map(|(_, d)| d.clone()).collect();
    write_ico(&ico_path, &ico_data, &ico_sizes);
    println!("  wrote {ico_path}");

    // Copy SVG source
    let svg_dest = format!("{out_dir}/ntfy-rs.svg");
    fs::copy(&svg_path, &svg_dest).expect("failed to copy SVG");
    println!("  wrote {svg_dest}");

    println!("Done! Icon set generated in {out_dir}/");
}

fn render(tree: &resvg::usvg::Tree, size: u32) -> resvg::tiny_skia::Pixmap {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap alloc");
    let scale = size as f32 / tree.size().width(); // assumes square viewBox
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap
}

fn encode_png(pixmap: &resvg::tiny_skia::Pixmap, size: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let w = BufWriter::new(&mut buf);
        let mut encoder = png::Encoder::new(w, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(pixmap.data())
            .expect("PNG write");
    }
    buf
}

/// Write a simple ICO file containing the given PNG images.
fn write_ico(path: &str, pngs: &[Vec<u8>], sizes: &[u32]) {
    let count = pngs.len() as u16;
    let mut f = BufWriter::new(fs::File::create(path).expect("create ICO"));

    // ICO header: reserved(2) + type(2) + count(2)
    f.write_all(&[0, 0, 1, 0]).unwrap(); // reserved=0, type=1 (ICO)
    f.write_all(&count.to_le_bytes()).unwrap();

    // Directory entries
    let mut offset: u32 = 6 + (count as u32) * 16; // header + directory
    for (i, &size) in sizes.iter().enumerate() {
        let w = if size >= 256 { 0u8 } else { size as u8 };
        let data_len = pngs[i].len() as u32;
        f.write_all(&[w, w, 0, 0, 1, 0, 32, 0]).unwrap(); // w,h,palette,reserved,colors,reserved,bpp
        f.write_all(&data_len.to_le_bytes()).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        offset += data_len;
    }

    // Image data
    for png in pngs {
        f.write_all(png).unwrap();
    }
}
