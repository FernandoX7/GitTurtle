#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = gitturtle_preview::metadata::iso_image_format(data);
    let _ = gitturtle_preview::metadata::is_svg(data);
    let _ = gitturtle_preview::metadata::is_image(data);
    let _ = gitturtle_preview::metadata::jpeg2000_format(data);
    let _ = gitturtle_preview::metadata::container(data);
    let _ = gitturtle_preview::detect_lfs_pointer(data);
    let _ = gitturtle_preview::declared_dimensions(data);
    let _ = gitturtle_preview::metadata::inspect(data, Path::new("fuzz.bin"));
    let _ = gitturtle_preview::decode_image(data, "fuzz.png", 128);
});
