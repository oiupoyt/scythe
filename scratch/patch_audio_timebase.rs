use std::fs;
fn main() {
    let mut code = fs::read_to_string("src/encoder.rs").unwrap();
    code = code.replace(
        "(*codec_ctx).sample_rate = sample_rate;",
        "(*codec_ctx).sample_rate = sample_rate;\n            (*codec_ctx).time_base = AVRational { num: 1, den: sample_rate };"
    );
    fs::write("src/encoder.rs", code).unwrap();
}
