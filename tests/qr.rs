use erisauth::qr;

fn decode(side: u32, pixels: Vec<u8>) -> String {
    let img =
        rqrr::PreparedImage::prepare_from_greyscale(side as usize, side as usize, |x, y| pixels[y * side as usize + x]);
    let mut img = img;
    let grids = img.detect_grids();
    assert_eq!(grids.len(), 1);
    grids[0].decode().unwrap().1
}

#[test]
fn the_raster_scans_back_to_the_data() {
    let data = "erisfiles://pair/eyJ2IjoxfQ";
    let (side, pixels) = qr::raster(data, 4).unwrap();
    assert_eq!(pixels.len(), (side * side) as usize);
    assert_eq!(decode(side, pixels), data);
}

#[test]
fn the_png_is_written_and_scans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pair.png");
    qr::write_png("hello ticket", &path, 6).unwrap();
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.width, info.height);
    buf.truncate(info.buffer_size());
    assert_eq!(decode(info.width, buf), "hello ticket");
}

#[test]
fn blocks_are_text_for_a_terminal() {
    let blocks = qr::to_blocks("hello").unwrap();
    assert!(blocks.ends_with('\n'));
    assert!(blocks.lines().count() > 10);
}

#[test]
fn svg_is_a_standalone_image() {
    let svg = qr::svg("hello").unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
}

#[test]
fn the_png_path_is_in_home_and_safe() {
    let path = qr::png_path("erisfiles", Some("my laptop/../x"));
    let name = path.file_name().unwrap().to_str().unwrap();
    assert_eq!(name, "erisfiles-pair-my-laptop----x.png");
}
