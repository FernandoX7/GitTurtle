#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    const EXTENSIONS: [&str; 6] = ["stl", "obj", "fbx", "glb", "3mf", "step"];
    if let Some((&selector, payload)) = data.split_first() {
        let ext = EXTENSIONS[usize::from(selector) % EXTENSIONS.len()];
        let name = format!("fuzz.{ext}");
        let _ = gitturtle_preview::model3d::decode_geometry(payload, &name, || Ok(()));
    }
});
